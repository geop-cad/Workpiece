//! Curve: a curve in space as a body of its own — through points, a
//! helix about an axis, or where two faces cross — to sweep along or to
//! build on.

use std::sync::Arc;

use anyhow::{Context, ensure};
use shapecraft_boolean::model::Walk;
use shapecraft_boolean::{Op, Solid, combine, read, read_sheet};
use shapecraft_core::math::Point3;
use shapecraft_core::widgets::{Choice, ReferenceList};
use shapecraft_core::world::DatumAxis;
use shapecraft_core::world::topo::{Body, BodyId, BodyKind, Curve3, Edge, Topo, Vertex};
use shapecraft_core::{
    Builder, EditContext, Measure, Operation, Position, Reference, ReferenceKind, Resolve, Widget,
};

use shapecraft_shape::ui;

/// What kind of curve.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum CurveWay {
    /// A smooth curve through picked points, in the order picked.
    #[default]
    Spline,
    /// A helix about an axis.
    Helix,
    /// Where two picked faces cross.
    Intersection,
    /// A picked curve body thrown onto a picked face, along an axis.
    Projection,
}

/// What the curve is of.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct CurveArgs {
    /// What kind of curve.
    #[arg(with = "way_field")]
    pub way: CurveWay,
    /// The axis of a helix.
    #[arg(
        label_by = "if self.way == CurveWay::Helix { \"Axis\" } else { \"Along\" }",
        when = "matches!(self.way, CurveWay::Helix | CurveWay::Projection)"
    )]
    pub axis: Option<Reference<DatumAxis>>,
    /// The points a spline goes through (vertices, frames), or the two
    /// faces that cross.
    #[arg(with = "picked_field", when = "self.way != CurveWay::Helix")]
    pub picked: Vec<shapecraft_core::Reference<shapecraft_core::Anything>>,
    /// Of a spline: where on the face picked at the same place in
    /// `picked` it goes through (as the face was when it was pointed
    /// at; the point of the face nearest that, as the face is now);
    /// nothing for what is a point by itself.
    #[serde(default)]
    #[arg(hidden)]
    pub spots: Vec<Option<[f64; 3]>>,
    /// Of a spline: how far off the faces it goes through its points on
    /// them — a cable's radius, laid along them.
    #[serde(default = "nothing")]
    #[arg(
        label = "Off the faces",
        when = "self.way == CurveWay::Spline && self.spots.iter().any(Option::is_some)"
    )]
    pub offset: Measure,
    /// The helix's radius.
    #[arg(when = "self.way == CurveWay::Helix")]
    pub radius: Measure,
    /// How far the helix advances in a turn.
    #[arg(row, when = "self.way == CurveWay::Helix")]
    pub pitch: Measure,
    /// How many turns it makes.
    #[arg(when = "self.way == CurveWay::Helix")]
    pub turns: Measure,
    /// How much its radius grows in a turn: a spiral.
    #[serde(default = "nothing")]
    #[arg(label = "Grows a turn", row, when = "self.way == CurveWay::Helix")]
    pub grow: Measure,
    /// Whether it is a left-handed helix.
    #[arg(flip, row, when = "self.way == CurveWay::Helix")]
    pub left: bool,
}

fn nothing() -> Measure {
    Measure::new(0.0)
}

impl Default for CurveArgs {
    fn default() -> Self {
        Self {
            way: CurveWay::Spline,
            picked: Vec::new(),
            spots: Vec::new(),
            offset: nothing(),
            axis: None,
            radius: Measure::new(10.0),
            pitch: Measure::new(5.0),
            turns: Measure::new(3.0),
            grow: nothing(),
            left: false,
        }
    }
}

/// Adds a curve in space as a wire body named after the operation's id,
/// its edges `{id}/e0`, `{id}/e1`, …
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Curve {
    /// The operation's id; also the body's name.
    pub id: String,
    /// What the curve is of.
    pub args: CurveArgs,
}

/// A smooth curve through `points`, in order: a cubic from each to the
/// next, leaving every point the way the points beside it lie.
pub fn through(points: &[Point3]) -> anyhow::Result<Curve3> {
    let n = points.len();
    ensure!(n >= 2, "a curve goes through two points or more");
    for pair in points.windows(2) {
        ensure!(
            pair[0].distance(pair[1]) > 0.0,
            "two points after each other are one place"
        );
    }
    // The way through each point: along the chord of its neighbours
    // (at the ends, so that the curve bends evenly into them).
    let way = |k: usize| -> Point3 {
        if n == 2 {
            points[1] - points[0]
        } else if k == 0 {
            (points[1] - points[0]) * 1.5 - (points[2] - points[0]) * 0.25
        } else if k == n - 1 {
            (points[n - 1] - points[n - 2]) * 1.5 - (points[n - 1] - points[n - 3]) * 0.25
        } else {
            (points[k + 1] - points[k - 1]) * 0.5
        }
    };
    let mut control = vec![points[0]];
    let mut knots = vec![0.0; 4];
    for k in 0..n - 1 {
        control.push(points[k] + way(k) / 3.0);
        control.push(points[k + 1] - way(k + 1) / 3.0);
        control.push(points[k + 1]);
        let at = (k + 1) as f64;
        knots.extend(if k + 2 == n { vec![at; 4] } else { vec![at; 3] });
    }
    Ok(Curve3::new(3, knots, control, None)?)
}

/// A curve, and the part of it that counts.
pub type Piece = (Arc<Curve3>, (f64, f64));

/// Where the sheets `a` and `b` cross: the pieces of the crossing that
/// are on both, each a curve and the part of it that counts.
pub fn crossing(a: &Solid, b: &Solid) -> anyhow::Result<Vec<Piece>> {
    let reach = {
        let ((lo, hi), (lo2, hi2)) = (a.extent(), b.extent());
        (hi.max(hi2) - lo.min(lo2)).length()
    };
    ensure!(reach > 0.0, "the faces have some size");
    // Each as a thin solid before it or behind it: where the two
    // overlap, the edge between the faces themselves is where the faces
    // cross. (Faces that end where they cross — two sides of a block —
    // have only what is behind both in common.)
    let all = |sheet: &Solid| -> Vec<usize> { (0..sheet.faces.len()).collect() };
    let mut found = Vec::new();
    for (front_a, front_b) in [(1.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0)] {
        let thin = |sheet: &Solid, side: f64| {
            shapecraft_shape::thick::thicken(sheet, &all(sheet), side * reach * 1e-3, false, true)
        };
        let (Ok(one), Ok(other)) = (thin(a, front_a), thin(b, front_b)) else {
            continue;
        };
        let Ok(common) = combine(Op::Intersect, &one, &other).map(|c| c.solid) else {
            continue;
        };
        // Which faces of what they have in common are pieces of the
        // sheets' own faces: by where they are.
        let on: Vec<[bool; 2]> = (0..common.faces.len())
            .map(|f| {
                let Some(p) = shapecraft_shape::surface::inside(&common, f) else {
                    return [false; 2];
                };
                [a, b].map(|sheet| {
                    (0..sheet.faces.len())
                        .any(|s| shapecraft_boolean::lies_on(sheet, s, p, reach * 1e-7))
                })
            })
            .collect();
        let mut beside: Vec<Vec<usize>> = vec![Vec::new(); common.edges.len()];
        for (f, face) in common.faces.iter().enumerate() {
            for step in face.loops.iter().flatten() {
                if let Walk::Edge { edge, .. } = step.walk {
                    beside[edge].push(f);
                }
            }
        }
        found = (0..common.edges.len())
            .filter(|&e| match beside[e][..] {
                [f, g] => (on[f][0] && on[g][1]) || (on[g][0] && on[f][1]),
                _ => false,
            })
            // (And is on both all along: not an edge across what the
            // faces were thickened by, where one of them ends.)
            .filter(|&e| {
                let edge = &common.edges[e];
                [0.25, 0.5, 0.75].iter().all(|share| {
                    let t = edge.range.0 + (edge.range.1 - edge.range.0) * share;
                    let p = edge.curve.point_at(t);
                    [a, b].iter().all(|sheet| {
                        (0..sheet.faces.len())
                            .any(|s| shapecraft_boolean::lies_on(sheet, s, p, reach * 1e-7))
                    })
                })
            })
            .map(|e| (common.edges[e].curve.clone(), common.edges[e].range))
            .collect();
        if !found.is_empty() {
            break;
        }
    }
    ensure!(!found.is_empty(), "the faces do not cross in a curve");
    Ok(found)
}

/// Adds `curves` (each with the part of it that counts) to `topo` as a
/// wire body called `name`, its edges `{name}/e0`, …; ends at one place
/// are one vertex.
pub fn write_wire(topo: &mut Topo, name: &str, curves: &[Piece]) -> anyhow::Result<BodyId> {
    ensure!(!curves.is_empty(), "there is no curve");
    let ends: Vec<[Point3; 2]> = curves
        .iter()
        .map(|(c, (t0, t1))| [c.point_at(*t0), c.point_at(*t1)])
        .collect();
    let size = ends
        .iter()
        .flatten()
        .fold(0.0_f64, |m, p| m.max(p.length()));
    let longest = ends.iter().map(|e| e[0].distance(e[1])).fold(0.0, f64::max);
    let tolerance = size.max(longest).max(1e-9) * 1e-9;
    let mut vertices: Vec<(Point3, shapecraft_core::world::topo::VertexId)> = Vec::new();
    let mut edges = Vec::new();
    for (k, (curve, (t0, t1))) in curves.iter().enumerate() {
        let mut at = |topo: &mut Topo, p: Point3| -> anyhow::Result<_> {
            if let Some((_, v)) = vertices
                .iter()
                .find(|(q, _)| q.distance(p) <= tolerance * 100.0)
            {
                return Ok(*v);
            }
            let v = topo.add_vertex(Vertex {
                point: p,
                tolerance: tolerance * 100.0,
            })?;
            vertices.push((p, v));
            Ok(v)
        };
        let (start, end) = (at(topo, ends[k][0])?, at(topo, ends[k][1])?);
        let curve = topo.add_curve((**curve).clone());
        let edge = topo.add_edge(Edge {
            curve,
            t0: *t0,
            t1: *t1,
            start,
            end,
            tolerance,
        })?;
        topo.set_name(edge, format!("{name}/e{k}"))?;
        edges.push(edge);
    }
    let body = topo.add_body(Body {
        kind: BodyKind::Wire,
        lumps: Vec::new(),
        free_edges: edges,
        free_vertices: Vec::new(),
    })?;
    topo.set_name(body, name)?;
    Ok(body)
}

/// The point of `face`'s surface nearest `to`, and the way out of the
/// body there.
fn nearest(face: &shapecraft_boolean::model::Face, to: Point3) -> (Point3, Point3) {
    let surface = &face.surface;
    let ((u0, u1), (v0, v1)) = surface.domain();
    // From the nearest of a grid of its points, closer step by step.
    const GRID: usize = 16;
    let mut best = (f64::INFINITY, u0, v0);
    for i in 0..=GRID {
        for j in 0..=GRID {
            let u = u0 + (u1 - u0) * i as f64 / GRID as f64;
            let v = v0 + (v1 - v0) * j as f64 / GRID as f64;
            let far = surface.point_at(u, v).distance(to);
            if far < best.0 {
                best = (far, u, v);
            }
        }
    }
    let (_, mut u, mut v) = best;
    for _ in 0..40 {
        let (p, du, dv) = surface.derivatives_at(u, v);
        let off = to - p;
        // (The step in u and v that takes the point nearest in the
        // tangent plane.)
        let (a, b, c) = (du.dot(du), du.dot(dv), dv.dot(dv));
        let (r, s) = (off.dot(du), off.dot(dv));
        let det = a * c - b * b;
        if det.abs() < 1e-300 {
            break;
        }
        let (su, sv) = ((r * c - s * b) / det, (s * a - r * b) / det);
        u = (u + su).clamp(u0, u1);
        v = (v + sv).clamp(v0, v1);
        if su.abs() <= 1e-14 * (u1 - u0) && sv.abs() <= 1e-14 * (v1 - v0) {
            break;
        }
    }
    let (p, du, dv) = surface.derivatives_at(u, v);
    let normal = du.cross(dv).normalize_or_zero();
    (p, if face.outward { normal } else { -normal })
}

/// The sheet of the one face called `name`, of whatever body has it.
fn face_sheet(topo: &Topo, name: &str) -> anyhow::Result<Solid> {
    for (id, body) in topo.bodies().iter() {
        let solid = match body.kind {
            BodyKind::Solid => read(topo, id),
            BodyKind::Sheet => read_sheet(topo, id),
            _ => continue,
        };
        let Ok(solid) = solid else {
            continue;
        };
        let found = solid
            .faces
            .iter()
            .position(|f| f.name.as_deref() == Some(name) || f.also.iter().any(|n| n == name));
        if let Some(face) = found {
            return Ok(shapecraft_shape::faces_of(&solid, &[face]));
        }
    }
    anyhow::bail!("there is no face {name:?}")
}

impl Operation for Curve {
    type Args = CurveArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &CurveArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut CurveArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        let args = &self.args;
        let whole = |curve: Curve3| {
            let range = curve.domain();
            vec![(Arc::new(curve), range)]
        };
        let curves = match args.way {
            CurveWay::Spline => {
                let world = builder.tracked();
                let offset = (*builder).measure(&args.offset, "the distance off the faces")?;
                let mut points = Vec::new();
                for (k, source) in args.picked.iter().enumerate() {
                    points.push(match args.spots.get(k).copied().flatten() {
                        // A point on a face: where the face is nearest
                        // to where it was put, and off it.
                        Some(at) => {
                            let sheet = face_sheet(&world.topo, source.name())?;
                            let (on, out) = nearest(&sheet.faces[0], Point3::from_array(at));
                            on + out * offset
                        }
                        None => Position::resolve(world, source)?.0,
                    });
                }
                whole(through(&points)?)
            }
            CurveWay::Helix => {
                let radius = (*builder).measure(&args.radius, "the radius")?;
                let pitch = (*builder).measure(&args.pitch, "the pitch")?;
                let turns = (*builder).measure(&args.turns, "the turns")?;
                ensure!(radius > 0.0, "a helix has some radius");
                let grow = (*builder).measure(&args.grow, "the growth")?;
                ensure!(
                    pitch != 0.0 || grow != 0.0,
                    "a helix advances as it turns, or a spiral grows"
                );
                let axis = args.axis.as_ref().context("pick the axis of the helix")?;
                let axis = builder.tracked().resolve(axis)?;
                let (origin, along) = (axis.origin(), axis.direction());
                let from = origin + along.any_orthonormal_vector() * radius;
                whole(shapecraft_shape::swept::helix(
                    origin, along, from, pitch, turns, grow, args.left,
                )?)
            }
            CurveWay::Projection => {
                let axis = args
                    .axis
                    .as_ref()
                    .context("pick the axis to throw it along")?;
                let along = builder.tracked().resolve(axis)?.direction();
                let topo = &builder.tracked().topo;
                // The curve and the face, whichever was picked first.
                let wire = |s: &&shapecraft_core::Reference<shapecraft_core::Anything>| {
                    let body = topo.resolve::<BodyId>(s.name()).and_then(|b| topo.body(b));
                    body.filter(|b| b.kind == BodyKind::Wire)
                };
                let curve = args.picked.iter().find_map(|s| wire(&s));
                let curve = curve.context("pick the curve to throw onto the face")?;
                let face = args.picked.iter().find(|s| wire(s).is_none());
                let face = face.context("pick the face to throw the curve onto")?;
                let onto = face_sheet(topo, face.name())?;
                let (lo, hi) = onto.extent();
                let mut found = Vec::new();
                for (k, &id) in curve.free_edges.iter().enumerate() {
                    let edge = topo.edge(id).context("an edge of the curve is gone")?;
                    let line = topo.curve(edge.curve).context("an edge without a curve")?;
                    // The edge carried along the axis, both ways, as far
                    // as the face is: where that wall crosses the face.
                    let from = line.point_at(edge.t0);
                    let far = (hi - lo).length() + lo.distance(from) + hi.distance(from);
                    let wall = shapecraft_shape::ruled(
                        line,
                        (edge.t0, edge.t1),
                        along * (2.0 * far),
                        &format!("{}/w{k}", self.id),
                    )?
                    .moved(&|p| p - along * far)?;
                    if let Ok(pieces) = crossing(&wall, &onto) {
                        found.extend(pieces);
                    }
                }
                ensure!(
                    !found.is_empty(),
                    "along the axis, the curve misses the face"
                );
                found
            }
            CurveWay::Intersection => {
                let [a, b] = &args.picked[..] else {
                    anyhow::bail!("pick the two faces that cross");
                };
                let topo = &builder.tracked().topo;
                crossing(&face_sheet(topo, a.name())?, &face_sheet(topo, b.name())?)?
            }
        };
        let wire = move |topo: &mut Topo, name: &str| write_wire(topo, name, &curves).map(|_| ());
        Ok(shapecraft_core::Made::body(&self.id, wire))
    }

    fn summary(&self) -> String {
        match self.args.way {
            CurveWay::Spline => format!("through {} points", self.args.picked.len()),
            CurveWay::Helix => format!("a helix of {} turns", self.args.turns),
            CurveWay::Intersection => "where two faces cross".to_owned(),
            CurveWay::Projection => "a curve thrown onto a face".to_owned(),
        }
    }
}

/// The field that chooses what curve it is; what was picked for
/// another kind goes with it.
fn way_field(args: &CurveArgs, _: &EditContext<'_>) -> Vec<Widget<CurveArgs>> {
    let ways = [
        (CurveWay::Spline, "Through points"),
        (CurveWay::Helix, "Helix or spiral"),
        (CurveWay::Intersection, "Where two faces cross"),
        (CurveWay::Projection, "A curve onto a face"),
    ];
    let labels = ways.iter().map(|w| w.1.to_owned()).collect();
    let chosen = ways.iter().position(|w| w.0 == args.way).unwrap_or(0);
    let set = ui::set(args, move |a, i: usize| {
        a.way = ways[i].0;
        a.picked.clear();
        a.spots.clear();
    });
    vec![Choice::new("Curve", labels, chosen, set).into()]
}

/// The box that takes what the curve is made of, of the kinds its way
/// takes.
fn picked_field(args: &CurveArgs, _: &EditContext<'_>) -> Vec<Widget<CurveArgs>> {
    let (label, kinds) = match args.way {
        CurveWay::Spline => ("Through", vec![ReferenceKind::Vertex, ReferenceKind::Point]),
        CurveWay::Intersection => ("Faces", vec![ReferenceKind::Face]),
        _ => (
            "Curve and face",
            vec![ReferenceKind::Body, ReferenceKind::Face],
        ),
    };
    let picked = shapecraft_core::Reference::sources(&args.picked);
    let set = ui::set(args, |a, picked: Vec<shapecraft_core::Source>| {
        // What went out of the box takes its place on a face with it.
        let was = shapecraft_core::Reference::sources(&a.picked);
        let mut spots = a.spots.clone();
        spots.resize(was.len(), None);
        if picked.len() + 1 == was.len() {
            let gone = (0..picked.len())
                .find(|&k| picked[k] != was[k])
                .unwrap_or(picked.len());
            spots.remove(gone);
        } else {
            spots.resize(picked.len(), None);
        }
        a.spots = spots;
        a.picked = shapecraft_core::Reference::each(picked)
    });
    let mut fields: Vec<Widget<CurveArgs>> = vec![
        ReferenceList::new(label, kinds, picked, set)
            .required()
            .into(),
    ];
    if args.way == CurveWay::Spline {
        // A point on a face, where the face is pointed at: the curve
        // goes through it next.
        let on_face = ui::set(args, |a, picked: shapecraft_core::widgets::Picked| {
            if !picked.instance.is_empty() {
                return;
            }
            let face = matches!(&picked.source, shapecraft_core::Source::Topology(name)
                if picked.within.topo.resolve::<shapecraft_core::world::topo::FaceId>(name).is_some());
            // (A vertex or a frame pointed at is a point by itself, as
            // in the box above.)
            let point = Position::resolve(&picked.within, &picked.source).is_ok();
            if face || point {
                a.spots.resize(a.picked.len(), None);
                a.picked.push(picked.source.into());
                a.spots.push(face.then(|| picked.at.to_array()));
            }
        });
        fields.push(
            shapecraft_core::widgets::ConnectorInput::new("Then on a face", None, on_face)
                .within(shapecraft_core::widgets::Scope::Outside(String::new()))
                .into(),
        );
    }
    fields
}
