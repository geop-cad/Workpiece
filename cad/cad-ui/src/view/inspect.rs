//! What is selected, measured: a body's volume, area and centre of
//! gravity; a face's area; an edge's length; a vertex's place; and of
//! two things, how far apart they are and at what angle.

use std::sync::{Arc, Mutex};

use workpiece_boolean::{face_area, measure, read, read_sheet};
use workpiece_core::Source;
use workpiece_core::math::Point3;
use workpiece_core::world::World;
use workpiece_core::world::topo::{BodyId, BodyKind, EdgeId, EntityId, FaceId, Topo, VertexId};
use workpiece_rasterize::tessellate;

use crate::picking::Candidate;

/// One thing measured: what, and how much.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// What is measured, e.g. `"Volume"`.
    pub label: String,
    /// Its value, written out.
    pub value: String,
}

/// A selected thing of the model.
#[derive(Clone, Copy)]
enum Thing {
    Body(BodyId),
    Face(FaceId),
    Edge(EdgeId),
    Vertex(VertexId),
}

fn number(x: f64) -> String {
    let x = if x.abs() < 5e-7 { 0.0 } else { x };
    let text = format!("{x:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_owned()
}

fn point(p: Point3) -> String {
    format!("{}, {}, {}", number(p.x), number(p.y), number(p.z))
}

fn reading(label: &str, value: String) -> Reading {
    Reading {
        label: label.to_owned(),
        value,
    }
}

/// The body `face` is of.
fn owner(topo: &Topo, face: FaceId) -> Option<BodyId> {
    topo.bodies()
        .ids()
        .find(|&b| topo.body_faces(b).is_some_and(|f| f.contains(&face)))
}

/// The area of `face`, from its surface and loops themselves.
/// The radius of the tightest bend of the surface `face` lies on, over
/// the part of it the face's edges reach; `None` for one that does not
/// bend.
fn tightest(topo: &Topo, face: FaceId) -> Option<f64> {
    use workpiece_core::math::Point2;
    let body = owner(topo, face)?;
    let solid = match topo.body(body)?.kind {
        BodyKind::Solid => read(topo, body).ok()?,
        BodyKind::Sheet => read_sheet(topo, body).ok()?,
        _ => return None,
    };
    let name = topo.name_of(face)?;
    let it = solid
        .faces
        .iter()
        .find(|f| f.name.as_deref() == Some(name))?;
    // Where in (u, v) the face is: the box around its edges there.
    let mut corners: Option<(Point2, Point2)> = None;
    for step in it.loops.iter().flatten() {
        let (from, to) = step.pcurve.domain();
        for k in 0..=8 {
            let p = step
                .pcurve
                .point_at(from + (to - from) * f64::from(k) / 8.0);
            corners = Some(corners.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))));
        }
    }
    let (lo, hi) = corners?;
    let surface = &it.surface;
    let ((u0, u1), (v0, v1)) = surface.domain();
    let (hu, hv) = ((u1 - u0) * 1e-4, (v1 - v0) * 1e-4);
    let mut most = 0.0_f64;
    for i in 0..=12 {
        for j in 0..=12 {
            // (Between the grid's lines: off the knots that tend to lie on them,
            // where a surface's derivatives may jump.)
            let u = (lo.x + (hi.x - lo.x) * (f64::from(i) + 0.37) / 13.0).clamp(u0 + hu, u1 - hu);
            let v = (lo.y + (hi.y - lo.y) * (f64::from(j) + 0.37) / 13.0).clamp(v0 + hv, v1 - hv);
            let (_, su, sv) = surface.derivatives_at(u, v);
            let Some(n) = su.cross(sv).try_normalize() else {
                continue;
            };
            // How the surface bends: its second derivatives, from the
            // first ones a little way off.
            let (_, su_u, sv_u) = surface.derivatives_at(u + hu, v);
            let (_, su_b, sv_b) = surface.derivatives_at(u - hu, v);
            let (_, _, sv_v) = surface.derivatives_at(u, v + hv);
            let (_, _, sv_w) = surface.derivatives_at(u, v - hv);
            let suu = (su_u - su_b) / (2.0 * hu);
            let suv = (sv_u - sv_b) / (2.0 * hu);
            let svv = (sv_v - sv_w) / (2.0 * hv);
            let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
            let (l, m, k) = (suu.dot(n), suv.dot(n), svv.dot(n));
            let det = e * g - f * f;
            if det <= 0.0 {
                continue;
            }
            let mean = (e * k - 2.0 * f * m + g * l) / (2.0 * det);
            let gauss = (l * k - m * m) / det;
            let spread = (mean * mean - gauss).max(0.0).sqrt();
            most = most.max((mean + spread).abs()).max((mean - spread).abs());
        }
    }
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    (most * size > 1e-6).then(|| 1.0 / most)
}

fn area(topo: &Topo, face: FaceId) -> Option<f64> {
    let body = owner(topo, face)?;
    let solid = match topo.body(body)?.kind {
        BodyKind::Solid => read(topo, body).ok()?,
        BodyKind::Sheet => read_sheet(topo, body).ok()?,
        _ => return None,
    };
    let name = topo.name_of(face)?;
    let index = solid
        .faces
        .iter()
        .position(|f| f.name.as_deref() == Some(name))?;
    Some(face_area(&solid, index))
}

/// Points along `edge`, closely spaced.
fn along(topo: &Topo, edge: EdgeId, steps: usize) -> Option<Vec<Point3>> {
    let e = topo.edge(edge)?;
    let curve = topo.curve(e.curve)?;
    Some(
        (0..=steps)
            .map(|k| curve.point_at(e.t0 + (e.t1 - e.t0) * k as f64 / steps as f64))
            .collect(),
    )
}

fn length(topo: &Topo, edge: EdgeId) -> Option<f64> {
    let points = along(topo, edge, 4096)?;
    Some(points.windows(2).map(|w| w[0].distance(w[1])).sum())
}

/// The radius of the circle `edge` is an arc of, if it is one.
fn radius(topo: &Topo, edge: EdgeId) -> Option<f64> {
    let points = along(topo, edge, 32)?;
    let (a, b, c) = (points[0], points[11], points[23]);
    let normal = (b - a).cross(c - a);
    let area = normal.length_squared();
    let size = a.distance(b).max(a.distance(c));
    if area <= 1e-18 * size.powi(4) {
        return None;
    }
    let (ab, ac) = (b - a, c - a);
    let centre = a
        + (normal.cross(ab) * ac.length_squared() + ac.cross(normal) * ab.length_squared())
            / (2.0 * area);
    let radius = centre.distance(a);
    let normal = normal.normalize();
    let round = points.iter().all(|p| {
        let d = *p - centre;
        (d.length() - radius).abs() <= 1e-9 * radius && d.dot(normal).abs() <= 1e-9 * radius
    });
    round.then_some(radius)
}

/// Points all over `thing`, to tell how far it is from another by.
fn points(topo: &Topo, thing: Thing) -> Vec<Point3> {
    let of_face = |face: FaceId| -> Vec<Point3> {
        tessellate::face(topo, face)
            .map(|mesh| mesh.triangles.iter().flat_map(|t| t.0).collect())
            .unwrap_or_default()
    };
    let mut all = match thing {
        Thing::Vertex(v) => topo.vertex(v).map(|v| vec![v.point]).unwrap_or_default(),
        Thing::Edge(e) => along(topo, e, 512).unwrap_or_default(),
        Thing::Face(f) => of_face(f),
        Thing::Body(b) => {
            let faces = topo.body_faces(b).unwrap_or_default();
            faces.into_iter().flat_map(of_face).collect()
        }
    };
    // (No more of them than tell it closely enough.)
    let step = all.len().div_ceil(3000).max(1);
    if step > 1 {
        all = all.into_iter().step_by(step).collect();
    }
    all
}

/// The way `thing` points, if it has one: along a straight edge, or
/// across a flat face (and then `true`).
fn way(topo: &Topo, thing: Thing) -> Option<(Point3, bool)> {
    match thing {
        Thing::Edge(e) => {
            let points = along(topo, e, 16)?;
            let (first, last) = (*points.first()?, *points.last()?);
            let way = (last - first).try_normalize()?;
            let straight = points
                .iter()
                .all(|p| (*p - first).reject_from(way).length() <= 1e-9 * first.distance(last));
            straight.then_some((way, false))
        }
        Thing::Face(f) => {
            let mesh = tessellate::face(topo, f)?;
            let first = mesh.triangles.first()?.1[0];
            let flat = mesh
                .triangles
                .iter()
                .all(|t| t.1.iter().all(|n| n.cross(first).length() <= 1e-9));
            flat.then_some((first, true))
        }
        _ => None,
    }
}

fn of_one(topo: &Topo, thing: Thing, density: Option<f64>) -> Vec<Reading> {
    match thing {
        Thing::Body(body) => {
            let Some(kind) = topo.body(body).map(|b| b.kind) else {
                return Vec::new();
            };
            if kind == BodyKind::Solid {
                let Ok(solid) = read(topo, body) else {
                    return Vec::new();
                };
                let m = measure(&solid);
                let mut all = vec![
                    reading("Volume", number(m.volume)),
                    reading("Surface area", number(m.area)),
                    reading("Centre of gravity", point(m.centre)),
                ];
                // (Of a part that says how dense its stuff is.)
                if let Some(density) = density {
                    all.insert(1, reading("Mass", number(m.volume * density)));
                }
                all
            } else if kind == BodyKind::Wire {
                // A curve: as long as its edges together.
                let edges = topo.body(body).map(|b| b.free_edges.clone());
                let long: f64 = edges
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|e| length(topo, *e))
                    .sum();
                vec![reading("Length", number(long))]
            } else {
                let faces = topo.body_faces(body).unwrap_or_default();
                let all: f64 = faces.iter().filter_map(|f| area(topo, *f)).sum();
                let mut made = vec![reading("Surface area", number(all))];
                // Where it is not closed: the edges of one face only.
                if let Ok(sheet) = read_sheet(topo, body) {
                    let mut walked = vec![0; sheet.edges.len()];
                    for step in sheet.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
                        if let workpiece_boolean::model::Walk::Edge { edge, .. } = step.walk {
                            walked[edge] += 1;
                        }
                    }
                    let open = (0..sheet.edges.len()).filter(|&e| walked[e] == 1);
                    let long: f64 = open
                        .clone()
                        .map(|e| {
                            let edge = &sheet.edges[e];
                            let at = |k: u32| {
                                let (from, to) = edge.range;
                                edge.curve
                                    .point_at(from + (to - from) * f64::from(k) / 4096.0)
                            };
                            (0..4096).map(|k| at(k).distance(at(k + 1))).sum::<f64>()
                        })
                        .sum();
                    let value = match open.count() {
                        0 => "none".to_owned(),
                        n => format!("{n}, {} long", number(long)),
                    };
                    made.push(reading("Open edges", value));
                }
                made
            }
        }
        Thing::Face(face) => {
            let mut all: Vec<Reading> = area(topo, face)
                .map(|a| reading("Area", number(a)))
                .into_iter()
                .collect();
            // How tightly it bends.
            match tightest(topo, face) {
                Some(radius) => all.push(reading("Tightest bend", format!("R {}", number(radius)))),
                None => all.push(reading("Tightest bend", "flat".to_owned())),
            }
            // How it leans from a pull along z: its draft, least and
            // most (a wall straight up has none; a floor has 90°).
            if let Some(mesh) = tessellate::face(topo, face) {
                let leans = mesh
                    .triangles
                    .iter()
                    .flat_map(|t| t.1)
                    .map(|n| n.z.clamp(-1.0, 1.0).asin().to_degrees().abs());
                let (least, most) = leans.fold((f64::INFINITY, 0.0_f64), |(lo, hi), a| {
                    (lo.min(a), hi.max(a))
                });
                if least.is_finite() {
                    let value = if (most - least) < 1e-6 {
                        format!("{}°", number(most))
                    } else {
                        format!("{}° to {}°", number(least), number(most))
                    };
                    all.push(reading("Draft from z", value));
                }
            }
            all
        }
        Thing::Edge(edge) => {
            let mut all: Vec<Reading> = length(topo, edge)
                .map(|l| reading("Length", number(l)))
                .into_iter()
                .collect();
            // An arc of a circle: its radius too.
            if let Some(radius) = radius(topo, edge) {
                all.push(reading("Radius", number(radius)));
            }
            all
        }
        Thing::Vertex(vertex) => topo
            .vertex(vertex)
            .map(|v| vec![reading("Position", point(v.point))])
            .unwrap_or_default(),
    }
}

/// Something measured, in the part it is of, where that is placed.
struct Placed<'a> {
    topo: &'a Topo,
    pose: workpiece_core::Pose,
    thing: Thing,
}

fn of_two(first: &Placed<'_>, second: &Placed<'_>) -> Vec<Reading> {
    let mut out = Vec::new();
    let (a, b) = (first.thing, second.thing);
    // (Each where its part is placed.)
    let spread = |it: &Placed<'_>| -> Vec<Point3> {
        let all = points(it.topo, it.thing);
        all.into_iter()
            .map(|p| it.pose.transform_point(p))
            .collect()
    };
    let facing = |it: &Placed<'_>| {
        way(it.topo, it.thing).map(|(n, flat)| (it.pose.transform_vector(n), flat))
    };
    let (pa, pb) = (spread(first), spread(second));
    let mut nearest: Option<(f64, Point3, Point3)> = None;
    for p in &pa {
        for q in &pb {
            let d = p.distance_squared(*q);
            if nearest.is_none_or(|n| d < n.0) {
                nearest = Some((d, *p, *q));
            }
        }
    }
    if let Some((d, p, q)) = nearest {
        let exact = matches!((a, b), (Thing::Vertex(_), Thing::Vertex(_)));
        let d = d.sqrt();
        // Two flat faces or straight edges side by side are as far
        // apart everywhere: told along what they have in common.
        let apart = match (facing(first), facing(second)) {
            (Some((n, true)), Some((m, true))) if n.cross(m).length() <= 1e-9 => {
                Some((q - p).dot(n).abs())
            }
            (Some((n, false)), Some((m, false))) if n.cross(m).length() <= 1e-9 => {
                Some((q - p).reject_from(n).length())
            }
            _ => None,
        };
        match apart {
            Some(apart) => out.push(reading("Distance", number(apart))),
            None if exact => out.push(reading("Distance", number(d))),
            None => out.push(reading("Distance", format!("≈ {}", number(d)))),
        }
        if exact {
            out.push(reading("ΔX, ΔY, ΔZ", point(q - p)));
        }
    }
    // Two bodies: whether they run into each other, and by how much.
    if let (Thing::Body(a), Thing::Body(b)) = (a, b)
        && let (Ok(a), Ok(b)) = (read(first.topo, a), read(second.topo, b))
        && let (Ok(a), Ok(b)) = (
            a.moved(&|p| first.pose.transform_point(p)),
            b.moved(&|p| second.pose.transform_point(p)),
        )
        && let Ok(common) = workpiece_boolean::combine(workpiece_boolean::Op::Intersect, &a, &b)
    {
        let shared = measure(&common.solid).volume;
        out.clear();
        if shared > 0.0 {
            out.push(reading(
                "Interference",
                format!("{} in common", number(shared)),
            ));
        } else {
            out.push(reading("Interference", "none".to_owned()));
            if let Some((d, _, _)) = nearest {
                out.push(reading("Clearance", format!("≈ {}", number(d.sqrt()))));
            }
        }
    }
    if let (Some((n, flat_a)), Some((m, flat_b))) = (facing(first), facing(second)) {
        let between = n.angle_between(m).to_degrees();
        let folded = between.min(180.0 - between);
        // (A line and a plane: from the plane, not from its normal.)
        let angle = if flat_a == flat_b {
            folded
        } else {
            90.0 - folded
        };
        out.push(reading("Angle", format!("{}°", number(angle))));
    }
    out
}

/// The solid bodies of everything placed in `world` (and in what is
/// placed there), each with the name of the part placed in `world`
/// that it belongs to, where `pose` puts it.
fn placed_solids(
    world: &World,
    pose: workpiece_core::Pose,
    of: Option<&str>,
    out: &mut Vec<(String, workpiece_boolean::Solid)>,
) {
    if let Some(of) = of {
        let topo = &world.topo;
        for (id, body) in topo.bodies().iter() {
            if body.kind != workpiece_core::world::topo::BodyKind::Solid {
                continue;
            }
            let moved = read(topo, id)
                .ok()
                .and_then(|s| s.moved(&|p| pose.transform_point(p)).ok());
            out.extend(moved.map(|s| (of.to_owned(), s)));
        }
    }
    for (name, instance) in &world.instances {
        placed_solids(
            &instance.world,
            pose * instance.pose,
            Some(of.unwrap_or(name)),
            out,
        );
    }
}

/// What an assembly measures as a whole: the volume and the centre of
/// gravity of all that is placed in `world`, and which placed parts
/// run into each other, with how much of them does.
pub fn of_assembly(world: &Arc<World>) -> Vec<Reading> {
    type Kept = Option<(Arc<World>, Vec<Reading>)>;
    static KEPT: Mutex<Kept> = Mutex::new(None);
    if world.instances.is_empty() {
        return Vec::new();
    }
    if let Ok(kept) = KEPT.lock()
        && let Some((of, readings)) = &*kept
        && Arc::ptr_eq(of, world)
    {
        return readings.clone();
    }
    let mut solids = Vec::new();
    placed_solids(world, workpiece_core::Pose::IDENTITY, None, &mut solids);
    let mut made = Vec::new();
    let (mut volume, mut centre) = (0.0, Point3::ZERO);
    for (_, solid) in &solids {
        let m = measure(solid);
        volume += m.volume;
        centre += m.centre * m.volume;
    }
    if volume > 0.0 {
        made.push(reading("Volume of all", number(volume)));
        made.push(reading("Centre of all", point(centre / volume)));
    }
    // Parts whose boxes overlap may run into each other: those are
    // looked at (the first few hundred pairs of them).
    let boxes: Vec<(Point3, Point3)> = solids.iter().map(|(_, s)| s.extent()).collect();
    let mut clashes: std::collections::BTreeMap<(String, String), f64> = Default::default();
    let mut looked = 0;
    // Points along every edge of each: how near two parts that do not
    // run into each other come is told from those.
    let along: Vec<Vec<Point3>> = solids
        .iter()
        .map(|(_, s)| {
            let edges = s.edges.iter().flat_map(|e| {
                (0..=8).map(move |k| {
                    let t = e.range.0 + (e.range.1 - e.range.0) * f64::from(k) / 8.0;
                    e.curve.point_at(t)
                })
            });
            edges.collect()
        })
        .collect();
    let mut nearest: Option<(f64, &str, &str)> = None;
    for i in 0..solids.len() {
        for j in i + 1..solids.len() {
            let ((a, one), (b, other)) = (&solids[i], &solids[j]);
            let apart = boxes[i].0.cmpgt(boxes[j].1).any() || boxes[j].0.cmpgt(boxes[i].1).any();
            if a == b || looked >= 300 {
                continue;
            }
            if apart {
                let gaps = along[i]
                    .iter()
                    .flat_map(|p| along[j].iter().map(move |q| p.distance(*q)));
                let gap = gaps.fold(f64::INFINITY, f64::min);
                if gap.is_finite() && nearest.is_none_or(|(least, ..)| gap < least) {
                    nearest = Some((gap, a, b));
                }
                continue;
            }
            looked += 1;
            let common = workpiece_boolean::combine(workpiece_boolean::Op::Intersect, one, other);
            let shared = common.map_or(0.0, |c| measure(&c.solid).volume);
            if shared > volume * 1e-12 {
                *clashes.entry((a.clone(), b.clone())).or_default() += shared;
            }
        }
    }
    if clashes.is_empty() {
        made.push(reading("Interference", "none".to_owned()));
    }
    if let Some((gap, a, b)) = nearest {
        let value = format!("≈ {} ({a}, {b})", number(gap));
        made.push(reading("Least clearance", value));
    }
    for ((a, b), shared) in clashes {
        made.push(reading(&format!("{a} in {b}"), number(shared)));
    }
    if let Ok(mut kept) = KEPT.lock() {
        *kept = Some((world.clone(), made.clone()));
    }
    made
}

/// What the selection measures, in `world`. With a `density` (the
/// part's parameter of that name: mass for each unit of volume), a
/// body's mass too.
pub fn readings(world: &Arc<World>, selection: &[Candidate], density: Option<f64>) -> Vec<Reading> {
    type Kept = Option<(Arc<World>, Vec<Candidate>, Option<u64>, Vec<Reading>)>;
    static KEPT: Mutex<Kept> = Mutex::new(None);
    if selection.is_empty() {
        return Vec::new();
    }
    if let Ok(kept) = KEPT.lock()
        && let Some((of, picked, dense, readings)) = &*kept
        && Arc::ptr_eq(of, world)
        && picked == selection
        && *dense == density.map(f64::to_bits)
    {
        return readings.clone();
    }
    let topo = &world.topo;
    // What is selected: of the part itself, or of a part placed in it
    // (or in one placed there, and so on).
    let placed: Vec<Placed<'_>> = selection
        .iter()
        .filter_map(|c| {
            let Candidate::Reference {
                instance,
                source: Source::Topology(name),
            } = c
            else {
                return None;
            };
            let (mut of, mut pose) = (&**world, workpiece_core::Pose::IDENTITY);
            for step in instance {
                let next = of.instances.get(step)?;
                pose = pose * next.pose;
                of = &*next.world;
            }
            let thing = match of.topo.referred(name)? {
                EntityId::Body(b) => Thing::Body(b),
                EntityId::Face(f) => Thing::Face(f),
                EntityId::Edge(e) => Thing::Edge(e),
                EntityId::Vertex(v) => Thing::Vertex(v),
                _ => return None,
            };
            Some(Placed {
                topo: &of.topo,
                pose,
                thing,
            })
        })
        .collect();
    let own = placed.iter().all(|p| std::ptr::eq(p.topo, topo));
    let things: Vec<Thing> = placed.iter().filter(|_| own).map(|p| p.thing).collect();
    let made = match placed[..] {
        [] => Vec::new(),
        // (Of a placed part: in that part's own coordinates.)
        [ref one] => of_one(one.topo, one.thing, density.filter(|_| own)),
        [ref a, ref b] => of_two(a, b),
        _ => {
            // Of many, what adds up.
            let sum = |label: &str, each: &dyn Fn(Thing) -> Option<f64>| {
                let all: Vec<f64> = things.iter().filter_map(|t| each(*t)).collect();
                (!all.is_empty()).then(|| reading(label, number(all.iter().sum())))
            };
            let volume = |t: Thing| match t {
                Thing::Body(b) => read(topo, b).ok().map(|s| measure(&s).volume),
                _ => None,
            };
            let faces = |t: Thing| match t {
                Thing::Face(f) => area(topo, f),
                _ => None,
            };
            let edges = |t: Thing| match t {
                Thing::Edge(e) => length(topo, e),
                _ => None,
            };
            [
                sum("Volume in all", &volume),
                sum("Area in all", &faces),
                sum("Length in all", &edges),
            ]
            .into_iter()
            .flatten()
            .collect()
        }
    };
    if let Ok(mut kept) = KEPT.lock() {
        *kept = Some((
            world.clone(),
            selection.to_vec(),
            density.map(f64::to_bits),
            made.clone(),
        ));
    }
    made
}

#[cfg(test)]
mod tests {
    use super::*;
    use workpiece_core::world::ORIGIN;
    use workpiece_core::{Measure, Operation, Reference, Standalone};
    use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs};

    #[test]
    fn what_is_selected_is_measured() -> anyhow::Result<()> {
        // A box 4 × 3 × 2 around the origin.
        let mut b = Standalone::new();
        AddBox {
            id: "box".into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(ORIGIN)),
                size: [4.0, 3.0, 2.0].map(Measure::new),
            },
        }
        .apply(&mut b)?;
        let world = Arc::new(b.world);
        let pick = |name: &str| Candidate::Reference {
            instance: Vec::new(),
            source: Source::Topology(name.to_owned()),
        };
        let read = |names: &[&str]| -> Vec<(String, String)> {
            let selection: Vec<Candidate> = names.iter().map(|n| pick(n)).collect();
            let all = readings(&world, &selection, None);
            all.into_iter().map(|r| (r.label, r.value)).collect()
        };
        let says = |names: &[&str], label: &str| {
            let all = read(names);
            all.into_iter().find(|r| r.0 == label).map(|r| r.1)
        };
        assert_eq!(says(&["box"], "Volume").as_deref(), Some("24"));
        // With a second box of 2 × 2 × 2 at the same place: what they
        // have in common.
        let mut both = Standalone {
            world: (*world).clone(),
            ..Standalone::new()
        };
        AddBox {
            id: "other".into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(ORIGIN)),
                size: [2.0, 2.0, 2.0].map(Measure::new),
            },
        }
        .apply(&mut both)?;
        let both = Arc::new(both.world);
        let clash = readings(&both, &[pick("box"), pick("other")], None);
        // With a density, a body has a mass.
        let heavy = readings(&world, &[pick("box")], Some(2.5));
        assert_eq!(
            (heavy[1].label.as_str(), heavy[1].value.as_str()),
            ("Mass", "60")
        );
        assert_eq!(clash[0].value, "8 in common");
        assert_eq!(says(&["box"], "Surface area").as_deref(), Some("52"));
        assert_eq!(
            says(&["box"], "Centre of gravity").as_deref(),
            Some("0, 0, 0")
        );
        assert_eq!(says(&["box/top"], "Area").as_deref(), Some("12"));
        assert_eq!(says(&["box/top"], "Draft from z").as_deref(), Some("90°"));
        assert_eq!(says(&["box/top"], "Tightest bend").as_deref(), Some("flat"));
        assert_eq!(says(&["box/front"], "Draft from z").as_deref(), Some("0°"));
        // Corner 0 is at (−x, −y, −z), corner 7 at (+x, +y, +z).
        assert_eq!(
            says(&["box/vertex0"], "Position").as_deref(),
            Some("-2, -1.5, -1")
        );
        let across = says(&["box/vertex0", "box/vertex7"], "Distance");
        assert_eq!(across.as_deref(), Some("5.385165"));
        // Two flat faces: how far apart, or at what angle.
        assert_eq!(
            says(&["box/top", "box/bottom"], "Distance").as_deref(),
            Some("2")
        );
        assert_eq!(
            says(&["box/top", "box/bottom"], "Angle").as_deref(),
            Some("0°")
        );
        assert_eq!(
            says(&["box/top", "box/front"], "Angle").as_deref(),
            Some("90°")
        );
        assert_eq!(
            says(&["box/top", "box/front", "box/left"], "Area in all").as_deref(),
            Some("26")
        );
        // A cylinder's rim is round: its radius is read too.
        let mut round = Standalone::new();
        workpiece_ops_basic_shapes::AddCylinder {
            id: "rod".into(),
            args: workpiece_ops_basic_shapes::AddCylinderArgs {
                frame: Some(Reference::datum(ORIGIN)),
                radius: Measure::new(1.5),
                height: Measure::new(2.0),
            },
        }
        .apply(&mut round)?;
        // Its side alone, as a surface, is open at both rims.
        let rod = workpiece_boolean::read(
            &round.world.topo,
            round.world.topo.resolve("rod").expect("the rod"),
        )?;
        let side = rod
            .faces
            .iter()
            .position(|f| f.name.as_deref() == Some("rod/side"));
        let skin = workpiece_shape::faces_of(&rod, &[side.expect("the side")]);
        let mut open = World::new();
        workpiece_boolean::write_sheet(&mut open.topo, &skin, "skin")?;
        let read_out = readings(&Arc::new(open), &[pick("skin")], None);
        let edges = read_out.iter().find(|r| r.label == "Open edges");
        // (Two rims of radius 1.5: 18.85 together.)
        let said = edges.map(|r| r.value.clone()).unwrap_or_default();
        assert!(
            said.starts_with("2, 18.8495") && said.ends_with(" long"),
            "{said}"
        );
        // Its side bends with its radius.
        let side = readings(&Arc::new(round.world.clone()), &[pick("rod/side")], None);
        let bend = side.iter().find(|r| r.label == "Tightest bend");
        assert_eq!(bend.map(|r| r.value.as_str()), Some("R 1.5"));
        let rim = readings(&Arc::new(round.world), &[pick("rod/bottom-rim")], None);
        let radius = rim
            .iter()
            .find(|r| r.label == "Radius")
            .map(|r| r.value.clone());
        assert_eq!(radius.as_deref(), Some("1.5"));
        // An edge is as long as the side it runs along.
        let long = says(&["box/edge0"], "Length").and_then(|l| l.parse::<f64>().ok());
        assert!(
            long.is_some_and(|l| [2.0, 3.0, 4.0].contains(&l)),
            "{long:?}"
        );
        Ok(())
    }
}
