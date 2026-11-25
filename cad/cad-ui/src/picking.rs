//! Picking on the model: what a ray hits, as references (or, while a step
//! is edited, as its handles), from the world and the handles themselves,
//! not from what is drawn.
//!
//! Candidates come in this order, and by distance along the ray within
//! each class:
//!
//! 1. the centers of datum frames and gizmos (to drag freely),
//! 2. axes (frame and datum axes, gizmo and handle arrows),
//! 3. planes and rings (datum planes, frames' inset planes, gizmo rings),
//! 4. vertices, 5. edges, 6. faces.
//!
//! Sketches and bodies are candidates only for a field that takes them:
//! a sketch by its curves (as an edge) or inside one of its areas (as a
//! face), a body by its faces.
//!
//! Datums are drawn as large as their world says ([`FRAME_SIZE`],
//! [`PLANE_SIZE`], [`AXIS_SIZE`] times its datum size); picking uses
//! the same.

use std::collections::{BTreeMap, HashMap};

use shapecraft_cad_rasterize::Pick;
use shapecraft_cad_rasterize::tessellate;
use shapecraft_cad_rasterize::world::{AXIS_SIZE, FRAME_SIZE, INSET, PLANE_SIZE};
use shapecraft_core::handles::shape::Shape;
use shapecraft_core::handles::{GizmoPart, Handle};
use shapecraft_core::math::{Axis, Point2, Point3};
use shapecraft_core::world::topo::EntityId;
use shapecraft_core::world::visibility::shown_topology;
use shapecraft_core::world::{Datum, Visibility, World};
use shapecraft_core::{Pose, Ray, ReferenceKind, Source};

/// Something under the pointer that can be picked.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Candidate {
    /// A reference: `source` in the world of the instance at `instance`
    /// (instance names from the picked world down; empty for that world).
    Reference {
        /// The path of instances it is in.
        instance: Vec<String>,
        /// The datum, frame axis or named topology.
        source: Source,
    },
    /// A piece of one of the edited step's handles.
    Handle {
        /// The handle's name.
        name: String,
        /// What it moves: a gizmo part, or `None` for a linear handle.
        part: Option<GizmoPart>,
    },
}

impl From<&Candidate> for Pick {
    fn from(c: &Candidate) -> Self {
        match c {
            Candidate::Reference { instance, source } => Pick::Entity {
                instance: instance.clone(),
                source: source.clone(),
            },
            Candidate::Handle { name, part } => Pick::Handle {
                name: name.clone(),
                part: *part,
            },
        }
    }
}

/// The classes candidates are ordered by.
/// A sketch, where sketches are asked for: before everything, as it
/// is drawn in front of everything then.
const SKETCH: u8 = 0;
const CENTER: u8 = 0;
const AXIS: u8 = 1;
const PLANE: u8 = 2;
const VERTEX: u8 = 3;
const EDGE: u8 = 4;
const FACE: u8 = 5;
/// A body, by its faces: behind the face itself where a field takes
/// both, so that pointing at a part gives one of its faces first.
const BODY: u8 = 6;

/// Collects hits, keeping each candidate's best (class, distance).
#[derive(Default)]
struct Hits(HashMap<Candidate, (u8, f64)>);

impl Hits {
    fn add(&mut self, candidate: impl FnOnce() -> Candidate, class: u8, t: Option<f64>) {
        let Some(t) = t else {
            return;
        };
        let best = self.0.entry(candidate()).or_insert((class, t));
        if (class, t) < *best {
            *best = (class, t);
        }
    }

    fn sorted(self) -> Vec<Candidate> {
        let mut hits: Vec<_> = self.0.into_iter().collect();
        hits.sort_by(|a, b| a.1.0.cmp(&b.1.0).then(a.1.1.total_cmp(&b.1.1)));
        hits.into_iter().map(|(c, _)| c).collect()
    }
}

fn hit_quad(ray: &Ray, [a, b, c, d]: [Point3; 4]) -> Option<f64> {
    ray.hit_triangle(a, b, c)
        .or_else(|| ray.hit_triangle(a, c, d))
}

/// The references in `world` (placed by `pose`, showing what `visibility`
/// does not hide) that `ray` hits and that resolve to one of `accepted`
/// (any reference if `accepted` is empty), in candidate order. Something
/// of a placed part counts as what it is seen as where the part is.
pub fn references(
    world: &World,
    pose: Pose,
    visibility: &Visibility,
    ray: &Ray,
    accepted: &[ReferenceKind],
) -> Vec<Candidate> {
    let mut hits = Hits::default();
    // Picking in the world's own coordinates.
    let Some(local) = place(ray, pose.inverse()) else {
        return Vec::new();
    };
    collect(&mut hits, world, &local, visibility, &mut Vec::new());
    if accepted.contains(&ReferenceKind::Profile) {
        profiles(&mut hits, world, &local, visibility);
    }
    if accepted.contains(&ReferenceKind::Body) {
        bodies(&mut hits, world, &local, visibility);
    }
    let accepts = |c: &Candidate| match c {
        Candidate::Reference { instance, source } => {
            // Something of a placed part is taken as seen where the
            // part is: a face of it as a plane, an edge as an axis.
            let source = Source::placed(instance, source.clone());
            accepted.is_empty() || accepted.iter().any(|k| k.accepts(world, &source))
        }
        Candidate::Handle { .. } => false,
    };
    hits.sorted().into_iter().filter(accepts).collect()
}

/// Where `ray` meets `source` of the instance at `instance` (names from
/// `world` down) — a vertex itself, the point of an edge nearest the ray,
/// where it goes into a face, a datum's origin — in that instance's
/// coordinates.
pub fn hit_point(world: &World, ray: &Ray, instance: &[String], source: &Source) -> Option<Point3> {
    let (mut within, mut local) = (world, *ray);
    for name in instance {
        let placed = within.instances.get(name)?;
        local = place(&local, placed.pose.inverse())?;
        within = &placed.world;
    }
    let topo = &within.topo;
    let off = |p: Point3| {
        let d = p - local.start();
        d.reject_from(local.direction()).length()
    };
    match source {
        Source::Datum(name) | Source::FrameAxis { frame: name, .. } => {
            Some(match within.datums.get(name)? {
                Datum::Frame(frame) => frame.origin(),
                Datum::Plane(plane) => plane.origin(),
                Datum::Axis(axis) => axis.origin(),
            })
        }
        Source::Profile(_) | Source::Placed { .. } => None,
        Source::Topology(name) => match topo.referred(name)? {
            EntityId::Vertex(v) => Some(topo.vertex(v)?.point),
            EntityId::Edge(e) => {
                let edge = topo.edge(e)?;
                let curve = topo.curve(edge.curve)?;
                (0..=256)
                    .map(|k| curve.point_at(edge.t0 + (edge.t1 - edge.t0) * f64::from(k) / 256.0))
                    .min_by(|a, b| off(*a).total_cmp(&off(*b)))
            }
            EntityId::Face(f) => tessellate::face(topo, f)?
                .triangles
                .iter()
                .filter_map(|([a, b, c], _)| local.hit_triangle(*a, *b, *c))
                .min_by(f64::total_cmp)
                .map(|t| local.at(t)),
            _ => None,
        },
    }
}

/// The shown sketches of `world` that `ray` hits: on a curve, or inside
/// an area.
fn profiles(hits: &mut Hits, world: &World, ray: &Ray, visibility: &Visibility) {
    for (name, profile) in world.shown_profiles(visibility) {
        let candidate = || Candidate::Reference {
            instance: Vec::new(),
            source: Source::Profile(name.clone()),
        };
        let pose = profile.plane.pose();
        let outlines: Vec<Vec<Point2>> = profile
            .edges
            .iter()
            .map(|e| e.curve.polyline(e.t0, e.t1))
            .collect();
        let on_curve = outlines
            .iter()
            .flat_map(|points| points.windows(2))
            .filter_map(|p| {
                let place = |p: Point2| pose.transform_point(p.extend(0.0));
                ray.hit_segment(place(p[0]), place(p[1]))
            })
            .min_by(f64::total_cmp);
        hits.add(candidate, SKETCH, on_curve);
        // Inside an area: inside an odd number of the outlines' turns.
        let plane = &profile.plane;
        let inside = ray
            .intersect_plane(plane.origin(), plane.normal())
            .filter(|&t| {
                let at = pose.inverse().transform_point(ray.at(t));
                let at = Point2::new(at.x, at.y);
                profile.regions.iter().any(|region| {
                    let within = |walk: &Vec<(usize, bool)>| {
                        let ring: Vec<Point2> = walk
                            .iter()
                            .flat_map(|&(e, reversed)| {
                                let mut piece = outlines[e].clone();
                                if reversed {
                                    piece.reverse();
                                }
                                piece
                            })
                            .collect();
                        contains(&ring, at)
                    };
                    within(&region.outer) && !region.holes.iter().any(within)
                })
            });
        hits.add(candidate, SKETCH, inside);
    }
}

/// Whether the polygon `ring` is around `p`, by the even-odd rule.
fn contains(ring: &[Point2], p: Point2) -> bool {
    let mut inside = false;
    for (k, &a) in ring.iter().enumerate() {
        let b = ring[(k + 1) % ring.len()];
        if (a.y > p.y) != (b.y > p.y) && a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) > p.x {
            inside = !inside;
        }
    }
    inside
}

/// What a ray is tried against of a world's topology: every edge as
/// points along it, every face as triangles. Made once for a world as
/// it was built (told apart by its [stamp](World::stamp)) and kept:
/// pointing at a part does not sample its faces again with every move
/// of the pointer.
#[derive(Default)]
struct Sampled {
    edges: HashMap<shapecraft_core::world::topo::EdgeId, Vec<Point3>>,
    faces: HashMap<shapecraft_core::world::topo::FaceId, Vec<[Point3; 3]>>,
}

fn sampled(world: &World) -> std::sync::Arc<Sampled> {
    use std::sync::{Arc, Mutex};
    static KEPT: Mutex<Vec<(u64, Arc<Sampled>)>> = Mutex::new(Vec::new());
    let make = || {
        let topo = &world.topo;
        let mut out = Sampled::default();
        for (id, edge) in topo.edges().iter() {
            if let Some(curve) = topo.curve(edge.curve) {
                out.edges.insert(id, curve.polyline(edge.t0, edge.t1));
            }
        }
        for id in topo.faces().ids() {
            if let Some(mesh) = tessellate::face(topo, id) {
                out.faces
                    .insert(id, mesh.triangles.iter().map(|t| t.0).collect());
            }
        }
        Arc::new(out)
    };
    // A world that was not built by the engine has no stamp to know it
    // by.
    if world.stamp == 0 {
        return make();
    }
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = kept.iter().position(|k| k.0 == world.stamp) {
        let found = kept.remove(at);
        kept.push(found.clone());
        return found.1;
    }
    let made = make();
    kept.push((world.stamp, made.clone()));
    if kept.len() > 64 {
        kept.remove(0);
    }
    made
}

/// The shown, named bodies of `world` that `ray` hits, by their faces.
fn bodies(hits: &mut Hits, world: &World, ray: &Ray, visibility: &Visibility) {
    let topo = &world.topo;
    let sampled = sampled(world);
    for body in topo.bodies().ids() {
        let Some(name) = topo.name_of(body) else {
            continue;
        };
        if visibility.bodies.contains(name) {
            continue;
        }
        let t = topo
            .body_faces(body)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|face| sampled.faces.get(&face))
            .flatten()
            .filter_map(|[a, b, c]| ray.hit_triangle(*a, *b, *c))
            .min_by(f64::total_cmp);
        let candidate = || Candidate::Reference {
            instance: Vec::new(),
            source: Source::Topology(name.to_owned()),
        };
        hits.add(candidate, BODY, t);
    }
}

/// `ray` moved by `pose`.
pub(crate) fn place(ray: &Ray, pose: Pose) -> Option<Ray> {
    Ray::new(
        pose.transform_point(ray.start()),
        pose.transform_vector(ray.direction()),
        ray.projection(),
    )
}

fn collect(
    hits: &mut Hits,
    world: &World,
    ray: &Ray,
    visibility: &Visibility,
    instance: &mut Vec<String>,
) {
    let reference = |source: Source| {
        let instance = instance.clone();
        move || Candidate::Reference { instance, source }
    };
    let size = visibility.datum_size.unwrap_or_else(|| world.datum_size());
    for (name, datum) in world.shown_datums(visibility) {
        let datum_ref = || reference(Source::Datum(name.clone()));
        match datum {
            Datum::Frame(frame) => {
                let o = frame.origin();
                hits.add(datum_ref(), CENTER, ray.hit_point(o));
                for axis in Axis::ALL {
                    let axis_ref = || {
                        reference(Source::FrameAxis {
                            frame: name.clone(),
                            axis,
                        })
                    };
                    let end = o + frame.direction(axis) * (FRAME_SIZE * size);
                    hits.add(axis_ref(), AXIS, ray.hit_segment(o, end));
                    let u = frame.direction(axis.next()) * (FRAME_SIZE * size);
                    let v = frame.direction(axis.next().next()) * (FRAME_SIZE * size);
                    let (lo, hi) = INSET;
                    let corners = [
                        o + u * lo + v * lo,
                        o + u * hi + v * lo,
                        o + u * hi + v * hi,
                        o + u * lo + v * hi,
                    ];
                    hits.add(axis_ref(), PLANE, hit_quad(ray, corners));
                }
            }
            Datum::Axis(axis) => {
                let (o, d) = (axis.origin(), axis.direction() * (AXIS_SIZE * size));
                hits.add(datum_ref(), AXIS, ray.hit_segment(o - d, o + d));
            }
            Datum::Plane(plane) => {
                let (o, x, y) = (
                    plane.origin(),
                    plane.x_dir() * (PLANE_SIZE * size),
                    plane.y_dir() * (PLANE_SIZE * size),
                );
                hits.add(
                    datum_ref(),
                    PLANE,
                    hit_quad(ray, [o - x - y, o + x - y, o + x + y, o - x + y]),
                );
            }
        }
    }

    let topo = &world.topo;
    let shown = shown_topology(topo, visibility);
    let sampled = sampled(world);
    let named = |id: shapecraft_core::world::topo::EntityId| {
        topo.name_of(id)
            .map(|n| reference(Source::Topology(n.to_owned())))
    };
    for &id in &shown.vertices {
        if let (Some(candidate), Some(v)) = (named(id.into()), topo.vertex(id)) {
            hits.add(candidate, VERTEX, ray.hit_point(v.point));
        }
    }
    for &id in &shown.edges {
        let (Some(candidate), Some(points)) = (named(id.into()), sampled.edges.get(&id)) else {
            continue;
        };
        let t = points
            .windows(2)
            .filter_map(|p| ray.hit_segment(p[0], p[1]))
            .min_by(f64::total_cmp);
        hits.add(candidate, EDGE, t);
    }
    for &id in &shown.faces {
        let (Some(candidate), Some(triangles)) = (named(id.into()), sampled.faces.get(&id)) else {
            continue;
        };
        let t = triangles
            .iter()
            .filter_map(|[a, b, c]| ray.hit_triangle(*a, *b, *c))
            .min_by(f64::total_cmp);
        hits.add(candidate, FACE, t);
    }

    for (name, inst) in world.shown_instances(visibility) {
        let Some(inner) = place(ray, inst.pose.inverse()) else {
            continue;
        };
        instance.push(name.clone());
        // Of a placed part, what shows: its bodies.
        collect(
            hits,
            &inst.world,
            &inner,
            &Visibility::placed(&inst.world, &shapecraft_core::world::Shades::new()),
            instance,
        );
        instance.pop();
    }
}

/// The pieces of `handles` (placed by `pose`) that `ray` hits, in
/// candidate order. `pixel` gives the size of a pixel in model units at a
/// (placed) point, as for drawing.
pub fn handles<M: 'static>(
    handles: &BTreeMap<String, Handle<M>>,
    pose: Pose,
    ray: &Ray,
    pixel: &dyn Fn(Point3) -> f64,
) -> Vec<Candidate> {
    let mut hits = Hits::default();
    let Some(ray) = place(ray, pose.inverse()) else {
        return Vec::new();
    };
    for (name, handle) in handles {
        let scale = pixel(pose.transform_point(handle.anchor()));
        for piece in handle.pieces(scale) {
            let candidate = || Candidate::Handle {
                name: name.clone(),
                part: piece.part,
            };
            let (class, t) = match piece.shape {
                Shape::Knob(at) => {
                    let class = if piece.part == Some(GizmoPart::Free) {
                        CENTER
                    } else {
                        AXIS
                    };
                    (class, ray.hit_point(at))
                }
                Shape::Arrow(from, to) => (AXIS, ray.hit_segment(from, to)),
                Shape::Ring {
                    center,
                    normal,
                    radius,
                } => (PLANE, ray.hit_circle(center, normal, radius)),
            };
            hits.add(candidate, class, t);
        }
    }
    hits.sorted()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shapecraft_core::{Operation, Projection, Standalone};
    use shapecraft_ops_basic_shapes::{AddBox, AddBoxArgs};

    fn reference(source: Source) -> Candidate {
        Candidate::Reference {
            instance: Vec::new(),
            source,
        }
    }

    fn topology(name: &str) -> Candidate {
        reference(Source::Topology(name.into()))
    }

    fn boxed() -> anyhow::Result<World> {
        let mut b = Standalone::new();
        AddBox {
            id: "box".into(),
            args: AddBoxArgs {
                frame: Some(shapecraft_core::Reference::datum("origin")),
                ..AddBoxArgs::default()
            },
        }
        .apply(&mut b)?;
        Ok(b.world)
    }

    fn down(x: f64, y: f64) -> Ray {
        let ortho = Projection::Orthographic { radius: 0.02 };
        Ray::new(Point3::new(x, y, 10.0), -Point3::Z, ortho).expect("ray")
    }

    #[test]
    fn classes_come_first_then_distance() -> anyhow::Result<()> {
        let world = boxed()?;
        let all = Visibility {
            datum_size: Some(1.0),
            ..Visibility::ALL
        };
        // Straight down through the origin: the origin frame's center
        // first, then its z axis, then (the plane it shows is seen
        // edge-on) the top and bottom faces, nearest first.
        let hits = references(&world, Pose::IDENTITY, &all, &down(0.0, 0.0), &[]);
        assert_eq!(hits[0], reference(Source::Datum("origin".into())));
        assert!(matches!(
            &hits[1],
            Candidate::Reference {
                source: Source::FrameAxis { axis: Axis::Z, .. },
                ..
            }
        ));
        let faces: Vec<_> = hits
            .iter()
            .filter(|c| **c == topology("box/top") || **c == topology("box/bottom"))
            .collect();
        assert_eq!(faces, [&topology("box/top"), &topology("box/bottom")]);
        // On the top's edge at x = 0.5 (and off the frame): the edge
        // before the face.
        let hits = references(&world, Pose::IDENTITY, &all, &down(0.5, -0.3), &[]);
        assert!(
            matches!(&hits[0], Candidate::Reference { source: Source::Topology(n), .. } if n.starts_with("box/edge"))
        );
        Ok(())
    }

    #[test]
    fn filters_and_visibility_drop_candidates() -> anyhow::Result<()> {
        let world = boxed()?;
        let all = Visibility {
            datum_size: Some(1.0),
            ..Visibility::ALL
        };
        // Planes only: the frame's XY plane (inset at 0.2–0.5) and the
        // planar faces; no axes, edges or vertices.
        let planes = references(
            &world,
            Pose::IDENTITY,
            &all,
            &down(0.3, 0.3),
            &[ReferenceKind::Plane],
        );
        assert_eq!(
            planes[0],
            reference(Source::FrameAxis {
                frame: "origin".into(),
                axis: Axis::Z
            })
        );
        assert!(planes.contains(&topology("box/top")));
        assert!(planes.iter().all(|c| !matches!(
            c,
            Candidate::Reference {
                source: Source::Datum(_),
                ..
            }
        )));
        // Hiding the frame and the box leaves nothing.
        let hidden = Visibility {
            datums: ["origin".to_owned()].into(),
            bodies: ["box".to_owned()].into(),
            ..Visibility::default()
        };
        assert!(references(&world, Pose::IDENTITY, &hidden, &down(0.3, 0.3), &[]).is_empty());
        Ok(())
    }
}
