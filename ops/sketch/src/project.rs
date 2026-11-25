//! Projecting the model onto the sketch plane: vertices become points,
//! edges become lines, arcs, circles or splines (what each is within its
//! tolerance, see [`recognize`]), faces their edges, and axes construction
//! lines. What is projected follows the model: every build projects it
//! again.

use anyhow::Context;
use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::topo::nurbs::{Recognized, recognize};
use shapecraft_core::world::topo::{Curve2, EdgeId, EntityId};
use shapecraft_core::world::{DatumAxis, DatumPlane};
use shapecraft_core::{Position, Resolve, Source, World};

use crate::model::{Entity, Id, Point, Shape, SketchArgs};

/// How long the projection of an axis is drawn, either way from its
/// origin.
const AXIS_REACH: f64 = 2.0;

/// The projection of one reference.
#[derive(Clone)]
pub(crate) struct Found {
    /// What it is the projection of.
    pub(crate) source: Source,
    /// What it is.
    pub(crate) shape: Recognized,
    /// How far the true curve may be from it.
    pub(crate) tolerance: f64,
    /// Whether it was not asked for: it lies in the sketch plane.
    pub(crate) auto: bool,
}

fn drop_onto(plane: &DatumPlane, p: Point3) -> Point2 {
    plane.pose().inverse().transform_point(p).truncate()
}

fn edge(world: &World, plane: &DatumPlane, id: EdgeId, source: Source) -> anyhow::Result<Found> {
    let topo = &world.topo;
    let edge = topo.edge(id).context("the edge is gone")?;
    let curve = topo.curve(edge.curve).context("the edge has no curve")?;
    // Dropping onto a plane is affine: the control points drop, the knots
    // and weights stay.
    let flat = Curve2::new(
        curve.degree(),
        curve.knots().to_vec(),
        curve
            .control_points()
            .iter()
            .map(|&p| drop_onto(plane, p))
            .collect(),
        Some(curve.weights().to_vec()),
    )?;
    let (shape, deviation) =
        recognize(&flat, edge.t0, edge.t1, edge.tolerance).context("the edge has no length")?;
    Ok(Found {
        source,
        shape,
        tolerance: edge.tolerance + deviation,
        auto: false,
    })
}

/// The projections of `source` onto `plane`: one, or one per edge of a
/// face.
pub(crate) fn project(
    world: &World,
    plane: &DatumPlane,
    source: &Source,
) -> anyhow::Result<Vec<Found>> {
    // Something of a placed part: projected in that part's own world, onto
    // the sketch plane as that part sees it; what it follows is the
    // placed source (so it moves with the part).
    if let Source::Placed {
        instance,
        source: inner,
    } = source
    {
        let placed = world
            .instances
            .get(instance)
            .with_context(|| format!("no part is placed as {instance:?}"))?;
        let back = placed.pose.inverse();
        let local = DatumPlane::new(
            back.transform_point(plane.origin()),
            back.transform_vector(plane.normal()),
            back.transform_vector(plane.x_dir()),
        )?;
        let mut found = project(&placed.world, &local, inner)?;
        for each in &mut found {
            each.source = Source::Placed {
                instance: instance.clone(),
                source: Box::new(each.source.clone()),
            };
        }
        return Ok(found);
    }
    let topo = &world.topo;
    if let Source::Topology(name) = source {
        match topo
            .referred(name)
            .with_context(|| format!("nothing is named {name:?}"))?
        {
            EntityId::Vertex(id) => {
                let vertex = topo.vertex(id).context("the vertex is gone")?;
                return Ok(vec![Found {
                    source: source.clone(),
                    shape: Recognized::Point(drop_onto(plane, vertex.point)),
                    tolerance: vertex.tolerance,
                    auto: false,
                }]);
            }
            EntityId::Edge(id) => return Ok(vec![edge(world, plane, id, source.clone())?]),
            EntityId::Face(id) => {
                let mut found = Vec::new();
                let mut seen: Vec<EdgeId> = Vec::new();
                let face = topo.face(id).context("the face is gone")?;
                let coedges = face
                    .loops
                    .iter()
                    .filter_map(|&l| topo.loop_(l))
                    .flat_map(|l| &l.coedges)
                    .filter_map(|&c| topo.coedge(c));
                for coedge in coedges {
                    let Some(id) = coedge.edge_id().filter(|e| !seen.contains(e)) else {
                        continue;
                    };
                    seen.push(id);
                    // Only named edges can be found again later.
                    if let Some(name) = topo.name_of(id) {
                        found.push(edge(world, plane, id, Source::Topology(name.to_owned()))?);
                    }
                }
                return Ok(found);
            }
            _ => anyhow::bail!("{name:?} cannot be projected"),
        }
    }
    if let Ok(axis) = DatumAxis::resolve(world, source) {
        let o = drop_onto(plane, axis.origin());
        let end = drop_onto(plane, axis.origin() + axis.direction() * AXIS_REACH);
        let shape = if o.distance(end) > 1e-9 {
            Recognized::Line(o * 2.0 - end, end)
        } else {
            Recognized::Point(o)
        };
        return Ok(vec![Found {
            source: source.clone(),
            shape,
            tolerance: 0.0,
            auto: false,
        }]);
    }
    // A datum plane: where it crosses the sketch plane (a plane square
    // to the sketch shows as that line).
    if let (Source::Datum(_), Ok(other)) = (source, DatumPlane::resolve(world, source)) {
        let way = plane.normal().cross(other.normal());
        anyhow::ensure!(
            way.length() > 1e-9,
            "the plane is parallel to the sketch's: it does not cross it"
        );
        let way = way.normalize();
        // The point of both planes nearest the sketch plane's origin.
        let (n1, n2) = (plane.normal(), other.normal());
        let (d1, d2) = (n1.dot(plane.origin()), n2.dot(other.origin()));
        let at = (n2.cross(way) * d1 + way.cross(n1) * d2) / way.dot(n1.cross(n2));
        let (a, b) = (
            drop_onto(plane, at - way * AXIS_REACH),
            drop_onto(plane, at + way * AXIS_REACH),
        );
        return Ok(vec![Found {
            source: source.clone(),
            shape: Recognized::Line(a, b),
            tolerance: 0.0,
            auto: false,
        }]);
    }
    let Position(p) = Position::resolve(world, source).context("it cannot be projected")?;
    Ok(vec![Found {
        source: source.clone(),
        shape: Recognized::Point(drop_onto(plane, p)),
        tolerance: 0.0,
        auto: false,
    }])
}

/// The projections of the named edges and vertices of `world` that lie in
/// `plane` (within their tolerances): what a sketch on the plane starts
/// with. Vertices at the ends of such edges come with the edges.
pub(crate) fn in_plane(world: &World, plane: &DatumPlane) -> Vec<Found> {
    let topo = &world.topo;
    let to_plane = plane.pose().inverse();
    let off = |p: Point3| to_plane.transform_point(p).z.abs();
    let mut found = Vec::new();
    let mut ends = Vec::new();
    let mut vertices = Vec::new();
    for (name, id) in topo.names().iter() {
        let source = Source::Topology(name.to_owned());
        match id {
            EntityId::Edge(id) => {
                let Some(e) = topo.edge(id) else { continue };
                let Some(curve) = topo.curve(e.curve) else {
                    continue;
                };
                let reach = e.tolerance.max(crate::model::DEFAULT_TOLERANCE);
                if curve.control_points().iter().all(|&p| off(p) <= reach)
                    && let Ok(mut flat) = edge(world, plane, id, source)
                {
                    flat.auto = true;
                    found.push(flat);
                    ends.extend([e.start, e.end]);
                }
            }
            EntityId::Vertex(id) => {
                let Some(v) = topo.vertex(id) else { continue };
                if off(v.point) <= v.tolerance.max(crate::model::DEFAULT_TOLERANCE) {
                    vertices.push((id, v, source));
                }
            }
            _ => {}
        }
    }
    for (id, vertex, source) in vertices {
        if !ends.contains(&id) {
            found.push(Found {
                source,
                shape: Recognized::Point(drop_onto(plane, vertex.point)),
                tolerance: vertex.tolerance,
                auto: true,
            });
        }
    }
    found
}

/// Where the plane cuts the solid bodies of `world`: the outlines of
/// what it cuts through them, as curves in the plane. (Each comes with
/// its body as its source, which is not what it follows: these are
/// drawn once, and are the sketch's own.)
pub(crate) fn sections(world: &World, plane: &DatumPlane) -> Vec<Found> {
    use shapecraft_boolean::model::Walk;
    use shapecraft_core::world::topo::BodyKind;
    let topo = &world.topo;
    let mut found = Vec::new();
    for (id, body) in topo.bodies().iter() {
        if body.kind != BodyKind::Solid {
            continue;
        }
        let (Some(name), Ok(solid)) = (topo.name_of(id), shapecraft_boolean::read(topo, id)) else {
            continue;
        };
        // All of the body behind the plane: its face in the plane is
        // the section.
        let (lo, hi) = solid.extent();
        let far = (hi - lo).length() + lo.distance(plane.origin()) + hi.distance(plane.origin());
        let (x, y, normal) = (plane.x_dir(), plane.y_dir(), plane.normal());
        let corner = plane.origin() - (x + y + normal) * far;
        let Ok(behind) = shapecraft_boolean::block(
            "section",
            corner,
            [x, y, normal],
            [far * 2.0, far * 2.0, far],
        ) else {
            continue;
        };
        let Ok(cut) =
            shapecraft_boolean::combine(shapecraft_boolean::Op::Intersect, &solid, &behind)
        else {
            continue;
        };
        let cut = cut.solid;
        let mut seen: Vec<usize> = Vec::new();
        for face in &cut.faces {
            if !face
                .name
                .as_deref()
                .is_some_and(|n| n.starts_with("section/f1"))
            {
                continue;
            }
            for step in face.loops.iter().flatten() {
                let Walk::Edge { edge, .. } = step.walk else {
                    continue;
                };
                if seen.contains(&edge) {
                    continue;
                }
                seen.push(edge);
                let e = &cut.edges[edge];
                let flat = Curve2::new(
                    e.curve.degree(),
                    e.curve.knots().to_vec(),
                    e.curve
                        .control_points()
                        .iter()
                        .map(|&p| drop_onto(plane, p))
                        .collect(),
                    Some(e.curve.weights().to_vec()),
                );
                let Ok(flat) = flat else {
                    continue;
                };
                if let Some((shape, deviation)) =
                    recognize(&flat, e.range.0, e.range.1, e.tolerance)
                {
                    found.push(Found {
                        source: Source::Topology(name.to_owned()),
                        shape,
                        tolerance: e.tolerance + deviation,
                        auto: false,
                    });
                }
            }
        }
    }
    found
}

/// Whether a constraint uses entity `id` or one of its points.
pub(crate) fn used(args: &SketchArgs, id: Id) -> bool {
    let own = args.points_of(id);
    args.constraints.values().any(|c| {
        let (points, entities) = c.kind.operands();
        entities.contains(&id) || points.iter().any(|p| own.contains(p))
    })
}

/// What `sections` found for the world with this stamp and this plane:
/// the plane's cut is worked out once, not for every build of the sketch.
type Cut = (u64, [u64; 6], std::sync::Arc<Vec<Found>>);
static CUT: std::sync::Mutex<Option<Cut>> = std::sync::Mutex::new(None);

fn cut_by(world: &World, plane: &DatumPlane) -> std::sync::Arc<Vec<Found>> {
    let (o, n) = (plane.origin(), plane.normal());
    let key = [o.x, o.y, o.z, n.x, n.y, n.z].map(f64::to_bits);
    if world.stamp != 0
        && let Ok(guard) = CUT.lock()
        && let Some((stamp, at, found)) = guard.as_ref()
        && *stamp == world.stamp
        && *at == key
    {
        return found.clone();
    }
    let found = std::sync::Arc::new(sections(world, plane));
    if world.stamp != 0
        && let Ok(mut guard) = CUT.lock()
    {
        *guard = Some((world.stamp, key, found.clone()));
    }
    found
}

/// Whether the sketch has a curve of its own (not offered) exactly where
/// `found` is.
fn drawn(args: &SketchArgs, found: &Recognized) -> bool {
    let near = |a: Point2, b: Point2| a.distance(b) < 1e-9;
    args.entities.iter().any(|(&id, e)| {
        let at = |p: &Id| args.at(*p);
        match (&e.shape, found) {
            (Shape::Line(a, b), Recognized::Line(p, q)) => {
                (near(at(a), *p) && near(at(b), *q)) || (near(at(a), *q) && near(at(b), *p))
            }
            (
                Shape::Circle { center, radius },
                Recognized::Circle {
                    center: c,
                    radius: r,
                },
            ) => near(at(center), *c) && (radius - r).abs() < 1e-9,
            (
                Shape::Arc { center, start, end },
                Recognized::Arc {
                    center: c,
                    start: s,
                    end: f,
                },
            ) => near(at(center), *c) && near(at(start), *s) && near(at(end), *f),
            _ => {
                let _ = id;
                false
            }
        }
    })
}

/// Offers where `plane` cuts the solid bodies as curves of the sketch
/// that are not drawn (see `Entity::section`): what was offered before
/// and nothing uses goes, what is cut now and not there already comes.
fn offer_sections(args: &mut SketchArgs, world: &World, plane: &DatumPlane) {
    let stale: Vec<crate::model::Item> = args
        .entities
        .iter()
        .filter(|(id, e)| e.section && !used(args, **id))
        .map(|(&id, _)| crate::model::Item::Entity(id))
        .collect();
    args.remove(&stale);
    for found in cut_by(world, plane).iter() {
        if drawn(args, &found.shape) {
            continue;
        }
        let mut found = found.clone();
        found.auto = true;
        let id = place(args, found);
        if let Some(entity) = args.entities.get_mut(&id) {
            entity.projected = None;
            entity.section = true;
        }
    }
}

/// Projects what lies in `plane` into the sketch, for a sketch that was
/// just put on the plane: what was projected so for another plane goes
/// (unless something uses it), what is projected already stays.
pub(crate) fn auto(args: &mut SketchArgs, world: &World, plane: &DatumPlane) {
    let stale: Vec<crate::model::Item> = args
        .entities
        .iter()
        .filter(|(id, e)| e.auto && !used(args, **id))
        .map(|(&id, _)| crate::model::Item::Entity(id))
        .collect();
    args.remove(&stale);
    for found in in_plane(world, plane) {
        let have = args.entities.values().any(|e| {
            e.projected
                .as_ref()
                .is_some_and(|p| same(world, p, &found.source))
        });
        if !have {
            place(args, found);
        }
    }
    offer_sections(args, world, plane);
}

/// Keeps what was projected unasked what lies in `plane` now: what no
/// longer does goes (unless something uses it), what does by now is
/// projected too. (The model before the sketch may have changed since
/// it was drawn, and what its edges are called with it.)
fn follow(args: &mut SketchArgs, world: &World, plane: &DatumPlane) {
    let now = in_plane(world, plane);
    let stale: Vec<crate::model::Item> = args
        .entities
        .iter()
        .filter(|(id, e)| {
            e.auto
                && !used(args, **id)
                && !now.iter().any(|found| {
                    e.projected
                        .as_ref()
                        .is_some_and(|p| same(world, p, &found.source))
                })
        })
        .map(|(&id, _)| crate::model::Item::Entity(id))
        .collect();
    args.remove(&stale);
    for found in now {
        let have = args.entities.values().any(|e| {
            e.projected
                .as_ref()
                .is_some_and(|p| same(world, p, &found.source))
        });
        if !have {
            place(args, found);
        }
    }
    offer_sections(args, world, plane);
}

/// The kinds of reference that can be projected.
pub(crate) fn kinds() -> Vec<shapecraft_core::ReferenceKind> {
    use shapecraft_core::ReferenceKind as K;
    vec![K::Vertex, K::Edge, K::Face, K::Axis, K::Point]
}

/// Adds `found` to the sketch: fixed points and the entity through
/// them — as a reference: construction geometry, which the sketch's
/// areas are not made of until it is converted (or said to be drawn).
pub(crate) fn place(args: &mut SketchArgs, found: Found) -> Id {
    let tolerance = found.tolerance;
    let fixed = |args: &mut SketchArgs, at: Point2| {
        let id = args.fresh();
        let point = Point {
            at,
            fixed: true,
            tolerance,
        };
        args.points.insert(id, point);
        id
    };
    let shape = match found.shape {
        Recognized::Point(p) => Shape::Point(fixed(args, p)),
        Recognized::Line(a, b) => Shape::Line(fixed(args, a), fixed(args, b)),
        Recognized::Arc { center, start, end } => Shape::Arc {
            center: fixed(args, center),
            start: fixed(args, start),
            end: fixed(args, end),
        },
        Recognized::Circle { center, radius } => Shape::Circle {
            center: fixed(args, center),
            radius,
        },
        Recognized::Spline(curve) => Shape::Spline {
            control: curve
                .control_points()
                .iter()
                .map(|&p| fixed(args, p))
                .collect(),
            degree: curve.degree(),
            knots: curve.knots().to_vec(),
            weights: curve.weights().to_vec(),
        },
    };
    let id = args.fresh();
    let entity = Entity {
        shape,
        construction: true,
        tolerance,
        projected: Some(found.source),
        auto: found.auto,
        section: false,
        axis: None,
    };
    args.entities.insert(id, entity);
    id
}

/// Projects everything projected in `args` again, moving its points to
/// where the model is now. Fails for a reference that is gone, or that
/// is no longer the kind of curve it was.
pub(crate) fn refresh(
    args: &mut SketchArgs,
    world: &World,
    plane: &DatumPlane,
) -> anyhow::Result<()> {
    if args.entities.values().any(|e| e.auto) {
        follow(args, world, plane);
    }
    let projected: Vec<(Id, Source)> = args
        .entities
        .iter()
        .filter_map(|(&id, e)| Some((id, e.projected.clone()?)))
        .collect();
    for (id, source) in projected {
        let what = || format!("projecting {source:?}");
        let found = project(world, plane, &source).map(|found| found.into_iter().next());
        // What was projected unasked goes with what it was the projection
        // of, if nothing was built on it.
        if !matches!(found, Ok(Some(_))) && args.entities[&id].auto && !used(args, id) {
            args.remove(&[crate::model::Item::Entity(id)]);
            continue;
        }
        let found = found.with_context(what)?.with_context(what)?;
        let changed = || {
            anyhow::anyhow!(
                "{source:?} is no longer the kind of curve that was projected: project it again"
            )
        };
        let set = |args: &mut SketchArgs, point: Id, at: Point2| {
            if let Some(p) = args.points.get_mut(&point) {
                p.at = at;
                p.tolerance = found.tolerance;
            }
        };
        let shape = args.entities[&id].shape.clone();
        match (shape, &found.shape) {
            (Shape::Point(p), Recognized::Point(at)) => set(args, p, *at),
            (Shape::Line(a, b), Recognized::Line(pa, pb)) => {
                set(args, a, *pa);
                set(args, b, *pb);
            }
            (
                Shape::Arc { center, start, end },
                Recognized::Arc {
                    center: c,
                    start: s,
                    end: e,
                },
            ) => {
                set(args, center, *c);
                set(args, start, *s);
                set(args, end, *e);
            }
            (Shape::Circle { center, .. }, Recognized::Circle { center: c, radius }) => {
                set(args, center, *c);
                if let Some(Shape::Circle { radius: r, .. }) =
                    args.entities.get_mut(&id).map(|e| &mut e.shape)
                {
                    *r = *radius;
                }
            }
            (Shape::Spline { control, .. }, Recognized::Spline(curve))
                if control.len() == curve.control_points().len() =>
            {
                for (&p, &at) in control.iter().zip(curve.control_points()) {
                    set(args, p, at);
                }
                if let Some(Shape::Spline {
                    knots,
                    weights,
                    degree,
                    ..
                }) = args.entities.get_mut(&id).map(|e| &mut e.shape)
                {
                    *knots = curve.knots().to_vec();
                    *weights = curve.weights().to_vec();
                    *degree = curve.degree();
                }
            }
            // (What was projected unasked is projected anew as what
            // it is by now.)
            _ if args.entities[&id].auto && !used(args, id) => {
                args.remove(&[crate::model::Item::Entity(id)]);
                place(args, found);
                continue;
            }
            _ => return Err(changed()),
        }
        if let Some(entity) = args.entities.get_mut(&id) {
            entity.tolerance = found.tolerance;
        }
    }
    Ok(())
}

/// Whether `a` and `b` point at the same thing in `world`: by the same
/// name, or by two names of one piece of a body (the one it has, and
/// one it is also known by — what a part saved earlier calls it).
fn same(world: &World, a: &Source, b: &Source) -> bool {
    if a == b {
        return true;
    }
    match (a, b) {
        (Source::Topology(a), Source::Topology(b)) => {
            let (a, b) = (world.topo.referred(a), world.topo.referred(b));
            a.is_some() && a == b
        }
        _ => false,
    }
}
