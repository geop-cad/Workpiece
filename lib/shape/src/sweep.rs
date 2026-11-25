//! Sweeping a profile into a body: along a line (extrude) or around an
//! axis (revolve). Every edge of the profile's outline sweeps a face,
//! every vertex an edge; flat caps close the ends.
//!
//! The faces are exact: a swept NURBS curve is a NURBS surface with the
//! curve's control points swept. What a line sweeps is a plane or a
//! cylinder (or, around an axis it leans to, a cone), an arc's a cylinder
//! (or a sphere or torus); booleans know planes and cylinders.

use std::collections::{BTreeMap, BTreeSet};

use crate::arcs::{arc_form, ellipse};
use anyhow::Context;
use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::profile::EdgeUse;
use shapecraft_core::world::topo::{
    Body, BodyId, BodyKind, Coedge, CoedgeId, Curve2, Curve3, Edge, EdgeId, EntityId, Face,
    FaceUse, Loop, Lump, NurbsSurface, Orientation, Shell, Topo, Vertex, VertexId,
};
use shapecraft_core::world::{AreaPick, Profile};

/// Whether `p` is inside the polygon `points` (even-odd).
fn inside(points: &[Point2], p: Point2) -> bool {
    let mut inside = false;
    for k in 0..points.len() {
        let (a, b) = (points[k], points[(k + 1) % points.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// The points along `walk` of `profile`, in walking order.
pub fn outline_points(profile: &Profile, walk: &[EdgeUse]) -> Vec<Point2> {
    let mut points = Vec::new();
    for &(e, reversed) in walk {
        let edge = &profile.edges[e];
        let mut piece = edge.curve.polyline(edge.t0, edge.t1);
        if reversed {
            piece.reverse();
        }
        piece.pop();
        points.extend(piece);
    }
    points
}

/// The region of `profile` that `p` is in, if any: the smallest whose
/// outline is around it and none of whose holes is.
pub fn region_at(profile: &Profile, p: Point2) -> Option<usize> {
    let within = |walk: &Vec<EdgeUse>| inside(&outline_points(profile, walk), p);
    profile
        .regions
        .iter()
        .enumerate()
        .filter(|(_, r)| within(&r.outer) && !r.holes.iter().any(within))
        .min_by(|a, b| a.1.area.total_cmp(&b.1.area))
        .map(|(k, _)| k)
}

/// The region of `profile` that `pick` names.
pub fn picked_region(profile: &Profile, pick: &AreaPick) -> anyhow::Result<usize> {
    match pick {
        AreaPick::Inside { inside } => region_at(profile, *inside).with_context(|| {
            format!(
                "no area of the sketch is around ({}, {}): the point is outside its closed curves",
                inside.x, inside.y
            )
        }),
        AreaPick::Bounded(anchors) => Ok(profile.region_at(anchors)?),
    }
}

/// The regions of `profile` to sweep: the ones `picked` names, or, with
/// none picked, the whole sketch — every region not inside an odd number
/// of outlines, so a circle in a rectangle is a hole in a plate.
pub fn regions(profile: &Profile, picked: &[AreaPick]) -> anyhow::Result<Vec<usize>> {
    if !picked.is_empty() {
        let mut regions = Vec::new();
        for pick in picked {
            let region = picked_region(profile, pick).context("a picked area")?;
            if !regions.contains(&region) {
                regions.push(region);
            }
        }
        return Ok(regions);
    }
    let outlines: Vec<Vec<Point2>> = profile
        .regions
        .iter()
        .map(|r| outline_points(profile, &r.outer))
        .collect();
    let size = outlines
        .iter()
        .flatten()
        .fold(0.0_f64, |m, p| m.max(p.length()))
        .max(1.0);
    Ok((0..profile.regions.len())
        .filter(|&k| {
            // A point just inside the region.
            let (a, b) = (outlines[k][0], outlines[k][1]);
            let probe = (a + b) / 2.0 + (b - a).normalize_or_zero().perp() * 1e-7 * size;
            let around = (0..outlines.len())
                .filter(|&other| other != k && inside(&outlines[other], probe))
                .count();
            around % 2 == 0
        })
        .collect())
}

/// The outline of `regions` of `profile` together: the loops around them,
/// them on the left. Edges between two of the regions are not on it.
pub fn outline(profile: &Profile, regions: &[usize]) -> anyhow::Result<Vec<Vec<EdgeUse>>> {
    let mut uses: BTreeSet<EdgeUse> = BTreeSet::new();
    for &r in regions {
        let region = profile.regions.get(r).context("no such area")?;
        uses.extend(region.outer.iter().chain(region.holes.iter().flatten()));
    }
    let all = uses.clone();
    uses.retain(|&(e, reversed)| !all.contains(&(e, !reversed)));
    let ends = |(e, reversed): EdgeUse| {
        let edge = &profile.edges[e];
        if reversed {
            (edge.end, edge.start)
        } else {
            (edge.start, edge.end)
        }
    };
    let mut loops = Vec::new();
    while let Some(&first) = uses.iter().next() {
        let mut walk = vec![first];
        uses.remove(&first);
        loop {
            let at = ends(walk[walk.len() - 1]).1;
            if at == ends(first).0 {
                break;
            }
            let mut onward = uses.iter().copied().filter(|&u| ends(u).0 == at);
            let next = onward.next().context("the areas' outline does not close")?;
            anyhow::ensure!(
                onward.next().is_none(),
                "areas that only touch at a point cannot be swept as one"
            );
            uses.remove(&next);
            walk.push(next);
        }
        loops.push(walk);
    }
    Ok(loops)
}

/// What a surface sweeps of `profile`: the outlines around `regions`
/// (closed, what can be capped), and every other curve of the sketch
/// by itself (open). With areas `picked`, only their outlines.
pub fn curves(profile: &Profile, picked: &[AreaPick]) -> anyhow::Result<[Vec<Vec<EdgeUse>>; 2]> {
    let regions = regions(profile, picked)?;
    let closed = if regions.is_empty() {
        Vec::new()
    } else {
        outline(profile, &regions)?
    };
    if !picked.is_empty() {
        return Ok([closed, Vec::new()]);
    }
    let around: BTreeSet<usize> = closed.iter().flatten().map(|u| u.0).collect();
    let open = (0..profile.edges.len())
        .filter(|e| !around.contains(e))
        .map(|e| vec![(e, false)])
        .collect();
    Ok([closed, open])
}

/// What sweeps walks of a profile, closed or not, into a body in the
/// place it is given.
pub type Sweep<'a> = &'a dyn Fn(&mut Topo, &[Vec<EdgeUse>], bool) -> anyhow::Result<BodyId>;

/// The sheets `swept` makes of the curves a surface sweeps of `profile`
/// (see [`curves`]) as one: `swept` is given the walks and whether they
/// are closed, and a place to make its body in.
pub fn sheet_of(
    profile: &Profile,
    picked: &[AreaPick],
    swept: Sweep<'_>,
) -> anyhow::Result<shapecraft_boolean::Solid> {
    let [closed, open] = curves(profile, picked)?;
    let mut sheets = Vec::new();
    for (walks, closed) in [(closed, true), (open, false)] {
        if walks.is_empty() {
            continue;
        }
        let mut scratch = Topo::new();
        let body = swept(&mut scratch, &walks, closed)?;
        sheets.push(shapecraft_boolean::read_sheet(&scratch, body)?);
    }
    anyhow::ensure!(!sheets.is_empty(), "the sketch has no curve to sweep");
    if sheets.len() == 1 {
        return Ok(sheets.remove(0));
    }
    let size = sheets
        .iter()
        .flat_map(|s| &s.vertices)
        .fold(1.0_f64, |m, v| m.max(v.point.length()));
    Ok(shapecraft_boolean::stitched(&sheets, size * 1e-9)?.0)
}

/// What to make of a sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Make {
    /// A solid: the swept faces and the caps.
    Solid,
    /// A sheet: the swept faces, and the caps if `caps`.
    Sheet {
        /// Whether the ends are closed.
        caps: bool,
    },
}

/// The frame a profile is placed in: its origin and its x and y
/// directions in space.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    /// Where its origin is.
    pub origin: Point3,
    /// Its x direction.
    pub x: Point3,
    /// Its y direction.
    pub y: Point3,
}

impl Frame {
    /// The frame of `profile`'s plane.
    pub fn of(profile: &Profile) -> Self {
        Self {
            origin: profile.plane.origin(),
            x: profile.plane.x_dir(),
            y: profile.plane.y_dir(),
        }
    }

    /// The plane's normal.
    pub fn normal(&self) -> Point3 {
        self.x.cross(self.y)
    }

    /// The point `p` of the plane, in space.
    pub fn place(&self, p: Point2) -> Point3 {
        self.origin + self.x * p.x + self.y * p.y
    }

    fn moved(&self, by: Point3) -> Self {
        Self {
            origin: self.origin + by,
            ..*self
        }
    }
}

/// A turn around an axis.
#[derive(Debug, Clone, Copy)]
pub struct Turn {
    /// A point of the axis.
    pub origin: Point3,
    /// The unit direction of the axis.
    pub axis: Point3,
}

impl Turn {
    /// The foot of `p` on the axis.
    pub fn foot(&self, p: Point3) -> Point3 {
        self.origin + self.axis * (p - self.origin).dot(self.axis)
    }

    fn vector(&self, v: Point3, angle: f64) -> Point3 {
        let along = self.axis * v.dot(self.axis);
        let across = v - along;
        along + across * angle.cos() + self.axis.cross(across) * angle.sin()
    }

    fn point(&self, p: Point3, angle: f64) -> Point3 {
        let foot = self.foot(p);
        foot + self.vector(p - foot, angle)
    }

    fn frame(&self, frame: &Frame, angle: f64) -> Frame {
        Frame {
            origin: self.point(frame.origin, angle),
            x: self.vector(frame.x, angle),
            y: self.vector(frame.y, angle),
        }
    }
}

/// Builds one body into a topology, everything named `{prefix}/…`.
struct Maker<'a> {
    topo: &'a mut Topo,
    prefix: String,
    faces: Vec<(FaceUse, Vec<EdgeId>)>,
}

impl<'a> Maker<'a> {
    fn new(topo: &'a mut Topo, prefix: &str) -> anyhow::Result<Self> {
        let taken = topo.lookup(prefix).is_some()
            || topo.names().iter().any(|(name, _)| {
                name.strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('/'))
            });
        anyhow::ensure!(!taken, "topology named {prefix:?} exists");
        Ok(Self {
            topo,
            prefix: prefix.to_owned(),
            faces: Vec::new(),
        })
    }

    fn name(&mut self, id: impl Into<EntityId>, name: &str) -> anyhow::Result<()> {
        self.topo.set_name(id, format!("{}/{name}", self.prefix))?;
        Ok(())
    }

    fn vertex(&mut self, point: Point3, tolerance: f64, name: &str) -> anyhow::Result<VertexId> {
        let id = self.topo.add_vertex(Vertex { point, tolerance })?;
        self.name(id, name)?;
        Ok(id)
    }

    fn edge(
        &mut self,
        curve: Curve3,
        range: Option<(f64, f64)>,
        ends: (VertexId, VertexId),
        tolerance: f64,
        name: &str,
    ) -> anyhow::Result<EdgeId> {
        let (t0, t1) = range.unwrap_or_else(|| curve.domain());
        let curve = self.topo.add_curve(curve);
        let id = self.topo.add_edge(Edge {
            curve,
            t0,
            t1,
            start: ends.0,
            end: ends.1,
            tolerance,
        })?;
        self.name(id, name)?;
        Ok(id)
    }

    /// A use of `edge` whose image in (u, v) is the line from `from` (at
    /// the edge's start) to `to`.
    fn coedge(
        &mut self,
        edge: EdgeId,
        forward: bool,
        from: Point2,
        to: Point2,
    ) -> anyhow::Result<CoedgeId> {
        let (t0, t1) = self
            .topo
            .edge(edge)
            .map(|e| (e.t0, e.t1))
            .context("no such edge")?;
        let pcurve = Curve2::new(1, vec![t0, t0, t1, t1], vec![from, to], None)?;
        self.coedge_on(edge, forward, pcurve)
    }

    fn coedge_on(
        &mut self,
        edge: EdgeId,
        forward: bool,
        pcurve: Curve2,
    ) -> anyhow::Result<CoedgeId> {
        let pcurve = self.topo.add_pcurve(pcurve);
        let orientation = if forward {
            Orientation::Forward
        } else {
            Orientation::Reversed
        };
        Ok(self
            .topo
            .add_coedge(Coedge::edge(edge, orientation, Some(pcurve)))?)
    }

    fn pole(&mut self, vertex: VertexId, from: Point2, to: Point2) -> anyhow::Result<CoedgeId> {
        let pcurve = self.topo.add_pcurve(Curve2::line(from, to)?);
        Ok(self
            .topo
            .add_coedge(Coedge::pole(vertex, 0.0, 1.0, Some(pcurve)))?)
    }

    fn face(
        &mut self,
        surface: NurbsSurface,
        loops: Vec<Vec<CoedgeId>>,
        tolerance: f64,
        outward: bool,
        name: &str,
    ) -> anyhow::Result<()> {
        let mut edges = Vec::new();
        for &c in loops.iter().flatten() {
            edges.extend(self.topo.coedge(c).and_then(Coedge::edge_id));
        }
        let surface = self.topo.add_surface(surface);
        let mut ids = Vec::new();
        for coedges in loops {
            ids.push(self.topo.add_loop(Loop { coedges })?);
        }
        let face = self.topo.add_face(Face {
            surface,
            loops: ids,
            tolerance,
        })?;
        self.name(face, name)?;
        let orientation = if outward {
            Orientation::Forward
        } else {
            Orientation::Reversed
        };
        self.faces.push((FaceUse { face, orientation }, edges));
        Ok(())
    }

    /// The body of the faces made: a shell for every set of faces that
    /// hang together by their edges.
    fn body(self, kind: BodyKind) -> anyhow::Result<BodyId> {
        anyhow::ensure!(!self.faces.is_empty(), "there is nothing to sweep");
        let mut shell_of: Vec<usize> = (0..self.faces.len()).collect();
        let mut first_at: BTreeMap<EdgeId, usize> = BTreeMap::new();
        for (f, (_, edges)) in self.faces.iter().enumerate() {
            for &e in edges {
                let other = *first_at.entry(e).or_insert(f);
                let (a, b) = (shell_of[f], shell_of[other]);
                if a != b {
                    for s in &mut shell_of {
                        if *s == a.max(b) {
                            *s = a.min(b);
                        }
                    }
                }
            }
        }
        let mut lumps = Vec::new();
        let roots: BTreeSet<usize> = shell_of.iter().copied().collect();
        for root in roots {
            let faces = (0..self.faces.len())
                .filter(|&f| shell_of[f] == root)
                .map(|f| self.faces[f].0)
                .collect();
            let shell = self.topo.add_shell(Shell { faces })?;
            lumps.push(self.topo.add_lump(Lump {
                shells: vec![shell],
            })?);
        }
        let body = self.topo.add_body(Body {
            kind,
            lumps,
            ..Body::default()
        })?;
        self.topo.set_name(body, self.prefix)?;
        Ok(body)
    }
}

/// What of a profile a sweep uses.
struct Used<'a> {
    profile: &'a Profile,
    walks: &'a [Vec<EdgeUse>],
    edges: BTreeSet<usize>,
    vertices: BTreeSet<usize>,
}

impl<'a> Used<'a> {
    fn new(profile: &'a Profile, walks: &'a [Vec<EdgeUse>]) -> Self {
        let edges: BTreeSet<usize> = walks.iter().flatten().map(|u| u.0).collect();
        let vertices = edges
            .iter()
            .flat_map(|&e| [profile.edges[e].start, profile.edges[e].end])
            .collect();
        Self {
            profile,
            walks,
            edges,
            vertices,
        }
    }

    /// The tolerance of vertex `v`, with room for the ends of the edges
    /// swept from it.
    fn vertex_tolerance(&self, v: usize) -> f64 {
        self.profile.vertices[v].tolerance * (1.0 + 1e-6) + 1e-12
    }

    /// Edge `e` of the profile, placed in `frame`.
    fn curve(&self, e: usize, frame: &Frame) -> anyhow::Result<Curve3> {
        let curve = &self.profile.edges[e].curve;
        let points = curve
            .control_points()
            .iter()
            .map(|&p| frame.place(p))
            .collect();
        Ok(Curve3::new(
            curve.degree(),
            curve.knots().to_vec(),
            points,
            Some(curve.weights().to_vec()),
        )?)
    }

    /// A flat face over the outline, in the profile's plane placed in
    /// `frame`, its outside along the frame's normal if `up`, else
    /// against it. `edge_at` gives the edge there of each profile edge.
    fn cap(
        &self,
        maker: &mut Maker<'_>,
        frame: &Frame,
        up: bool,
        edge_at: &dyn Fn(usize) -> EdgeId,
        name: &str,
    ) -> anyhow::Result<()> {
        let mut lo = Point2::INFINITY;
        let mut hi = Point2::NEG_INFINITY;
        for walk in self.walks {
            for p in outline_points(self.profile, walk) {
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        let pad = Point2::splat((hi - lo).length() * 0.02 + 1e-9);
        let (lo, hi) = (lo - pad, hi + pad);
        let at = |x: f64, y: f64| frame.place(Point2::new(x, y));
        // Seen from outside, (u, v) turn left: against the normal, u and
        // v swap.
        let surface = if up {
            NurbsSurface::bilinear(
                at(lo.x, lo.y),
                at(hi.x, lo.y),
                at(lo.x, hi.y),
                at(hi.x, hi.y),
            )
        } else {
            NurbsSurface::bilinear(
                at(lo.x, lo.y),
                at(lo.x, hi.y),
                at(hi.x, lo.y),
                at(hi.x, hi.y),
            )
        }?;
        let uv = |p: Point2| {
            let q = (p - lo) / (hi - lo);
            if up { q } else { Point2::new(q.y, q.x) }
        };
        let mut loops = Vec::new();
        let mut tolerance = f64::INFINITY;
        for walk in self.walks {
            let mut coedges = Vec::new();
            for &(e, reversed) in walk {
                let edge = &self.profile.edges[e];
                tolerance = tolerance.min(edge.tolerance);
                let points = edge.curve.control_points().iter().map(|&p| uv(p)).collect();
                let pcurve = Curve2::new(
                    edge.curve.degree(),
                    edge.curve.knots().to_vec(),
                    points,
                    Some(edge.curve.weights().to_vec()),
                )?;
                // Against the normal the loop runs the other way.
                coedges.push(maker.coedge_on(edge_at(e), reversed != up, pcurve)?);
            }
            if !up {
                coedges.reverse();
            }
            loops.push(coedges);
        }
        maker.face(surface, loops, tolerance, true, name)
    }
}

/// Sweeps the `walks` of `profile` along its plane's normal, from the
/// distance `from` to the distance `to`, into the body `id`. With
/// `closed`, the walks are loops around what is swept (it on their left)
/// and can be capped.
pub fn extrude(
    topo: &mut Topo,
    id: &str,
    profile: &Profile,
    walks: &[Vec<EdgeUse>],
    closed: bool,
    (from, to): (f64, f64),
    make: Make,
) -> anyhow::Result<BodyId> {
    anyhow::ensure!(
        (to - from).abs() > 1e-9 && from.is_finite() && to.is_finite(),
        "there is no length to extrude along"
    );
    let used = Used::new(profile, walks);
    let base = Frame::of(profile);
    let normal = base.normal();
    let (low, high) = (from.min(to), from.max(to));
    let (low_name, high_name) = if from < to {
        ("start", "end")
    } else {
        ("end", "start")
    };
    let (bottom, top) = (base.moved(normal * low), base.moved(normal * high));
    let mut maker = Maker::new(topo, id)?;

    let mut vertices = BTreeMap::new();
    let mut rails = BTreeMap::new();
    for &v in &used.vertices {
        let vertex = &profile.vertices[v];
        let tolerance = used.vertex_tolerance(v);
        let (a, b) = (bottom.place(vertex.point), top.place(vertex.point));
        let ends = (
            maker.vertex(a, tolerance, &format!("{low_name}/{}", vertex.name))?,
            maker.vertex(b, tolerance, &format!("{high_name}/{}", vertex.name))?,
        );
        let rail = maker.edge(
            Curve3::line(a, b)?,
            None,
            ends,
            vertex.tolerance.min(1e-7),
            &format!("along/{}", vertex.name),
        )?;
        vertices.insert(v, ends);
        rails.insert(v, rail);
    }
    let mut edges = BTreeMap::new();
    for &e in &used.edges {
        let edge = &profile.edges[e];
        let range = Some((edge.t0, edge.t1));
        let (start, end) = (vertices[&edge.start], vertices[&edge.end]);
        let low = maker.edge(
            used.curve(e, &bottom)?,
            range,
            (start.0, end.0),
            edge.tolerance,
            &format!("{low_name}/{}", edge.name),
        )?;
        let high = maker.edge(
            used.curve(e, &top)?,
            range,
            (start.1, end.1),
            edge.tolerance,
            &format!("{high_name}/{}", edge.name),
        )?;
        edges.insert(e, (low, high));
    }

    // Each edge sweeps a face: u along the edge, v from bottom to top.
    for &(e, reversed) in walks.iter().flatten() {
        let edge = &profile.edges[e];
        let curve = &edge.curve;
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for (&p, &w) in curve.control_points().iter().zip(curve.weights()) {
            points.extend([bottom.place(p), top.place(p)]);
            weights.extend([w, w]);
        }
        let surface = NurbsSurface::new(
            curve.degree(),
            1,
            curve.knots().to_vec(),
            vec![0.0, 0.0, 1.0, 1.0],
            curve.control_points().len(),
            2,
            points,
            Some(weights),
        )?;
        let (t0, t1) = (edge.t0, edge.t1);
        let uv = Point2::new;
        let (low, high) = edges[&e];
        let coedges = vec![
            maker.coedge(low, true, uv(t0, 0.0), uv(t1, 0.0))?,
            maker.coedge(rails[&edge.end], true, uv(t1, 0.0), uv(t1, 1.0))?,
            maker.coedge(high, false, uv(t0, 1.0), uv(t1, 1.0))?,
            maker.coedge(rails[&edge.start], false, uv(t0, 0.0), uv(t0, 1.0))?,
        ];
        // The outside is to the right of the walk around the area.
        let tolerance = edge.tolerance.min(1e-7);
        let name = format!("side/{}", edge.name);
        maker.face(surface, vec![coedges], tolerance, !reversed, &name)?;
    }
    let capped = closed && make != Make::Sheet { caps: false };
    if capped {
        used.cap(&mut maker, &bottom, false, &|e| edges[&e].0, low_name)?;
        used.cap(&mut maker, &top, true, &|e| edges[&e].1, high_name)?;
    }
    maker.body(match make {
        Make::Solid => BodyKind::Solid,
        Make::Sheet { .. } => BodyKind::Sheet,
    })
}

/// Sweeps the `walks` of `profile` around `turn`'s axis (which lies in
/// the profile's plane), from the angle `start` through `angle` (2π for
/// all the way around), into the body `id`.
#[allow(clippy::too_many_arguments)]
pub fn revolve(
    topo: &mut Topo,
    id: &str,
    profile: &Profile,
    walks: &[Vec<EdgeUse>],
    closed: bool,
    turn: Turn,
    (start, angle): (f64, f64),
    make: Make,
) -> anyhow::Result<BodyId> {
    let tau = std::f64::consts::TAU;
    anyhow::ensure!(
        angle > 1e-9 && angle.is_finite(),
        "there is no angle to revolve through"
    );
    let angle = angle.min(tau);
    let full = angle >= tau - 1e-9;
    let used = Used::new(profile, walks);
    let base = Frame::of(profile);
    anyhow::ensure!(
        turn.axis.dot(base.normal()).abs() <= 1e-9
            && (turn.origin - base.origin).dot(base.normal()).abs() <= 1e-7,
        "the axis has to lie in the sketch's plane"
    );
    let first = turn.frame(&base, start);
    let last = turn.frame(&base, start + angle);
    // Which side of the axis the profile is on: positive where turning
    // moves it along the plane's normal.
    let side_of = |p: Point2| {
        let p = base.place(p);
        turn.axis.cross(p - turn.foot(p)).dot(base.normal())
    };
    let mut side = 0.0_f64;
    for walk in walks {
        for p in outline_points(profile, walk) {
            let s = side_of(p);
            anyhow::ensure!(
                s.abs() <= 1e-7 || side.abs() <= 1e-7 || (s > 0.0) == (side > 0.0),
                "the profile crosses the axis"
            );
            if s.abs() > side.abs() {
                side = s;
            }
        }
    }
    anyhow::ensure!(side != 0.0, "the profile lies on the axis");
    let positive = side > 0.0;
    let on_axis = |v: usize| {
        let vertex = &profile.vertices[v];
        side_of(vertex.point).abs() <= vertex.tolerance + 1e-9
    };
    let along_axis = |e: usize| {
        let edge = &profile.edges[e];
        let middle = edge.curve.point_at((edge.t0 + edge.t1) / 2.0);
        on_axis(edge.start) && on_axis(edge.end) && side_of(middle).abs() <= edge.tolerance + 1e-9
    };
    let mut maker = Maker::new(topo, id)?;

    // Vertices at the first and the last angle (one for both on the
    // axis, or all the way around), and the ring each sweeps.
    let mut vertices = BTreeMap::new();
    let mut rings = BTreeMap::new();
    for &v in &used.vertices {
        let vertex = &profile.vertices[v];
        let tolerance = used.vertex_tolerance(v);
        let a = first.place(vertex.point);
        if on_axis(v) {
            let only = maker.vertex(a, tolerance, &format!("axis/{}", vertex.name))?;
            vertices.insert(v, (only, only));
            continue;
        }
        let from = maker.vertex(a, tolerance, &format!("start/{}", vertex.name))?;
        let to = if full {
            from
        } else {
            let b = last.place(vertex.point);
            maker.vertex(b, tolerance, &format!("end/{}", vertex.name))?
        };
        let foot = turn.foot(a);
        let ring = ellipse(foot, a - foot, turn.axis.cross(a - foot), angle);
        let ring = maker.edge(
            ring,
            None,
            (from, to),
            vertex.tolerance.min(1e-7),
            &format!("ring/{}", vertex.name),
        )?;
        vertices.insert(v, (from, to));
        rings.insert(v, ring);
    }
    let capped = !full && closed && make != Make::Sheet { caps: false };
    let mut edges = BTreeMap::new();
    for &e in &used.edges {
        let edge = &profile.edges[e];
        let range = Some((edge.t0, edge.t1));
        let (a, b) = (vertices[&edge.start], vertices[&edge.end]);
        if along_axis(e) {
            // It sweeps nothing; the caps share it.
            if capped {
                let name = format!("axis/{}", edge.name);
                let only = maker.edge(
                    used.curve(e, &first)?,
                    range,
                    (a.0, b.0),
                    edge.tolerance,
                    &name,
                )?;
                edges.insert(e, (only, only));
            }
            continue;
        }
        let name = format!("start/{}", edge.name);
        let from = maker.edge(
            used.curve(e, &first)?,
            range,
            (a.0, b.0),
            edge.tolerance,
            &name,
        )?;
        let to = if full {
            from
        } else {
            let name = format!("end/{}", edge.name);
            maker.edge(
                used.curve(e, &last)?,
                range,
                (a.1, b.1),
                edge.tolerance,
                &name,
            )?
        };
        edges.insert(e, (from, to));
    }

    // Each edge sweeps a face: u along the edge, v around the axis.
    let (circle, knots) = arc_form(angle);
    for &(e, reversed) in walks.iter().flatten() {
        if along_axis(e) {
            continue;
        }
        let edge = &profile.edges[e];
        let curve = &edge.curve;
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for (&p, &w) in curve.control_points().iter().zip(curve.weights()) {
            let p = first.place(p);
            let foot = turn.foot(p);
            let (x, y) = (p - foot, turn.axis.cross(p - foot));
            for (c, cw) in &circle {
                points.push(foot + x * c.x + y * c.y);
                weights.push(w * cw);
            }
        }
        let surface = NurbsSurface::new(
            curve.degree(),
            2,
            curve.knots().to_vec(),
            knots.clone(),
            curve.control_points().len(),
            circle.len(),
            points,
            Some(weights),
        )?;
        let (t0, t1) = (edge.t0, edge.t1);
        let uv = Point2::new;
        let (from, to) = edges[&e];
        let mut coedges = vec![maker.coedge(from, true, uv(t0, 0.0), uv(t1, 0.0))?];
        coedges.push(match rings.get(&edge.end) {
            Some(&ring) => maker.coedge(ring, true, uv(t1, 0.0), uv(t1, 1.0))?,
            None => maker.pole(vertices[&edge.end].0, uv(t1, 0.0), uv(t1, 1.0))?,
        });
        coedges.push(maker.coedge(to, false, uv(t0, 1.0), uv(t1, 1.0))?);
        coedges.push(match rings.get(&edge.start) {
            Some(&ring) => maker.coedge(ring, false, uv(t0, 0.0), uv(t0, 1.0))?,
            None => maker.pole(vertices[&edge.start].0, uv(t0, 1.0), uv(t0, 0.0))?,
        });
        // The surface's normal is the outside where turning moves the
        // profile along its plane's normal and the walk goes along the
        // edge, or neither.
        let tolerance = edge.tolerance.min(1e-7);
        let name = format!("side/{}", edge.name);
        maker.face(
            surface,
            vec![coedges],
            tolerance,
            positive != reversed,
            &name,
        )?;
    }
    if capped {
        // The body is ahead of the first plane and behind the last.
        used.cap(&mut maker, &first, !positive, &|e| edges[&e].0, "start")?;
        used.cap(&mut maker, &last, positive, &|e| edges[&e].1, "end")?;
    }
    maker.body(match make {
        Make::Solid => BodyKind::Solid,
        Make::Sheet { .. } => BodyKind::Sheet,
    })
}

/// The corner patch that is the same seen from each of its three sides,
/// as a point for each (u, t) of [`corner`]'s own patch — whose three
/// sides it shares, with what lies beyond each of them bending on into
/// it as the blend that ends there bends. `None` if the section does
/// not run the same from both its ends.
///
/// With x, y, z how far a point is from the corner's three faces, each
/// as a share of the corner's size, the section in the plane z = 1 is
/// (x, y) = (B(s), A(s)), from (0, 1) to (1, 0), and A(s) = B(1 − s).
/// The patch is where ψ(x) + ψ(y) + ψ(z) = 1, for a ψ that falls from 1
/// to nothing with ψ(B(s)) + ψ(A(s)) = 1 all along the section (so that
/// the patch ends in it), is A itself where the section leaves a face
/// (to the fourth order nothing, so that the patch runs on into the
/// blend with its curvature) and 1 − B where it arrives at the other.
fn even_corner(
    centre: Point3,
    axes: [Point3; 3],
    section: &Curve2,
) -> Option<impl Fn(f64, f64) -> Point3> {
    let (t0, t1) = section.domain();
    let hv = section.control_points().to_vec();
    let weights = section.weights().to_vec();
    let n = hv.len();
    // (In one piece: its shares are then sums of what is not less than
    // nothing, and as close where they come to nothing as anywhere.)
    if n != section.degree() + 1 {
        return None;
    }
    let r = hv[0].y;
    // (B, A, 1 − B) at the share `s` of the section.
    let at = move |s: f64| {
        let s = s.clamp(0.0, 1.0);
        let (mut x, mut y, mut left, mut under) = (0.0, 0.0, 0.0, 0.0);
        let mut choose = 1.0;
        for (i, (p, w)) in hv.iter().zip(&weights).enumerate() {
            let share = choose * s.powi(i as i32) * (1.0 - s).powi((n - 1 - i) as i32) * w;
            // (What is nothing but for the last bits of the arithmetic
            // is nothing.)
            let some = |x: f64| if x.abs() <= 1e-11 * r { 0.0 } else { x };
            x += share * some(r - p.y);
            y += share * some(r - p.x);
            left += share * some(p.y);
            under += share;
            choose *= (n - 1 - i) as f64 / (i + 1) as f64;
        }
        (x / under / r, y / under / r, left / under / r)
    };
    for k in 0..=8 {
        let s = f64::from(k) / 8.0;
        let ((b, a, _), (b_back, a_back, _)) = (at(s), at(1.0 - s));
        if (a - b_back).abs() > 1e-9 || (b - a_back).abs() > 1e-9 {
            return None;
        }
    }
    let psi = {
        let at = at.clone();
        move |s: f64| {
            let (_, a, left) = at(s);
            let ease = |s: f64| s.powi(4) * (35.0 - 84.0 * s + 70.0 * s * s - 20.0 * s.powi(3));
            a * ease(s) + left * ease(1.0 - s)
        }
    };
    // Where ψ is `y`: it falls all the way.
    let back = {
        let psi = psi.clone();
        move |y: f64| {
            let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
            for _ in 0..64 {
                let mid = (lo + hi) / 2.0;
                if psi(mid) > y {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            (lo + hi) / 2.0
        }
    };
    let [a0, a1, a2] = axes;
    Some(move |u: f64, t: f64| {
        let (u, t) = ((u - t0) / (t1 - t0), (t - t0) / (t1 - t0));
        let rest = psi(1.0 - t);
        let (s0, s1) = (back(rest * psi(u)), back(rest * psi(1.0 - u)));
        let (x, y, z) = (at(s0).0, at(s1).0, at(t).0);
        centre + (a0 * (1.0 - x) + a1 * (1.0 - y) + a2 * (1.0 - z)) * r
    })
}

/// The solid between a corner patch and the three planes through
/// `centre` across `axes` (unit, square to each other): the part of a
/// rounded corner that stays, as seen from the ball's centre.
///
/// `section` is how each of the three blends that meet at the corner
/// runs in its own plane, as (distance from the third axis, height along
/// it): from (0, r) to (r, 0). The patch is that section turned about
/// the third axis *along the same section* instead of along a circle:
/// `centre + v(t)·a₂ + h(t)/r·(v(u)·a₀ + h(u)·a₁)`. It comes to a point
/// on the third axis, and along each of its three sides it is the end of
/// one blend, with that blend's tangent plane: a ball's eighth for a
/// circular section, and as smooth a corner for any other.
pub fn corner(
    topo: &mut Topo,
    id: &str,
    centre: Point3,
    axes: [Point3; 3],
    section: &Curve2,
) -> anyhow::Result<BodyId> {
    let [a0, a1, a2] = axes;
    let (t0, t1) = section.domain();
    let (knots, weights) = (section.knots().to_vec(), section.weights().to_vec());
    let hv = section.control_points();
    let n = hv.len();
    let r = hv[0].y;
    anyhow::ensure!(
        r > 0.0 && hv[0].x.abs() <= 1e-9 * r && hv[n - 1].y.abs() <= 1e-9 * r,
        "a corner's section runs from its axis to its foot"
    );
    let degree = section.degree();
    // The patch: u along the section in the plane across the third
    // axis, t along the section down from the point.
    let mut points = Vec::with_capacity(n * n);
    let mut both = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            let across = a0 * hv[i].y + a1 * hv[i].x;
            points.push(centre + a2 * hv[j].y + across * (hv[j].x / r));
            both.push(weights[i] * weights[j]);
        }
    }
    let patch = NurbsSurface::new(
        degree,
        degree,
        knots.clone(),
        knots.clone(),
        n,
        n,
        points,
        Some(both),
    )?;
    // A section that is no conic turned along itself is not the same
    // corner seen from each of its three sides: there the patch is the
    // one that is ([`even_corner`]), fitted over the same splines.
    let patch = match even_corner(centre, axes, section) {
        Some(even) if degree > 2 => crate::thick::refit(&patch, &even, r * 1e-8).unwrap_or(patch),
        _ => patch,
    };
    let curve = |x: Point3, y: Point3| {
        Curve3::new(
            degree,
            knots.clone(),
            hv.iter().map(|p| centre + x * p.x + y * p.y).collect(),
            Some(weights.clone()),
        )
    };
    let flat = |x: Point3, y: Point3| {
        let far = 2.0 * r;
        NurbsSurface::new(
            1,
            1,
            vec![0.0, 0.0, far, far],
            vec![0.0, 0.0, far, far],
            2,
            2,
            vec![
                centre,
                centre + y * far,
                centre + x * far,
                centre + (x + y) * far,
            ],
            None,
        )
    };
    let on_flat =
        |points: Vec<Point2>| Curve2::new(degree, knots.clone(), points, Some(weights.clone()));
    let tolerance = 1e-9 * r.max(1.0);
    let mut maker = Maker::new(topo, id)?;
    // (A vertex holds the ends of its edges, with their tolerance.)
    let middle = maker.vertex(centre, tolerance * 4.0, "centre")?;
    let feet = [
        maker.vertex(centre + a0 * r, tolerance * 4.0, "foot0")?,
        maker.vertex(centre + a1 * r, tolerance * 4.0, "foot1")?,
        maker.vertex(centre + a2 * r, tolerance * 4.0, "foot2")?,
    ];
    let mut spokes = Vec::new();
    for (k, axis) in axes.iter().enumerate() {
        let line = Curve3::line(centre, centre + *axis * r)?;
        spokes.push(maker.edge(
            line,
            None,
            (middle, feet[k]),
            tolerance,
            &format!("spoke{k}"),
        )?);
    }
    // The sides: in the plane of a₁ and a₂ (from the point down to the
    // second foot), of a₀ and a₂ (down to the first), and across a₂
    // (from the first foot to the second).
    let side1 = maker.edge(curve(a1, a2)?, None, (feet[2], feet[1]), tolerance, "side1")?;
    let side0 = maker.edge(curve(a0, a2)?, None, (feet[2], feet[0]), tolerance, "side0")?;
    let rim = {
        let across = Curve3::new(
            degree,
            knots.clone(),
            hv.iter().map(|p| centre + a0 * p.y + a1 * p.x).collect(),
            Some(weights.clone()),
        )?;
        maker.edge(across, None, (feet[0], feet[1]), tolerance, "rim")?
    };
    let uv = Point2::new;
    let handed = a0.cross(a1).dot(a2) > 0.0;
    // The patch, around: the point, down the second side, back along
    // the rim, up the first side.
    {
        let at = patch.point_at((t0 + t1) / 2.0, (t0 + t1) / 2.0);
        let (_, du, dv) = patch.derivatives_at((t0 + t1) / 2.0, (t0 + t1) / 2.0);
        let outward = du.cross(dv).dot(at - centre) > 0.0;
        let coedges = vec![
            maker.pole(feet[2], uv(t0, t0), uv(t1, t0))?,
            maker.coedge(side1, true, uv(t1, t0), uv(t1, t1))?,
            maker.coedge(rim, false, uv(t0, t1), uv(t1, t1))?,
            maker.coedge(side0, false, uv(t0, t0), uv(t0, t1))?,
        ];
        maker.face(patch, vec![coedges], tolerance, outward, "patch")?;
    }
    // The three flat sides, each with the solid behind it.
    let line = |maker: &mut Maker<'_>, k: usize, forward: bool, to: Point2| {
        maker.coedge(spokes[k], forward, Point2::ZERO, to)
    };
    {
        // Across a₂: (a₀, a₁).
        let coedges = vec![
            line(&mut maker, 0, true, uv(r, 0.0))?,
            maker.coedge_on(
                rim,
                true,
                on_flat(hv.iter().map(|p| uv(p.y, p.x)).collect())?,
            )?,
            line(&mut maker, 1, false, uv(0.0, r))?,
        ];
        maker.face(flat(a0, a1)?, vec![coedges], tolerance, !handed, "across2")?;
    }
    {
        // Across a₀: (a₁, a₂).
        let coedges = vec![
            line(&mut maker, 1, true, uv(r, 0.0))?,
            maker.coedge_on(side1, false, on_flat(hv.to_vec())?)?,
            line(&mut maker, 2, false, uv(0.0, r))?,
        ];
        maker.face(flat(a1, a2)?, vec![coedges], tolerance, !handed, "across0")?;
    }
    {
        // Across a₁: (a₂, a₀).
        let coedges = vec![
            line(&mut maker, 2, true, uv(r, 0.0))?,
            maker.coedge_on(
                side0,
                true,
                on_flat(hv.iter().map(|p| uv(p.y, p.x)).collect())?,
            )?,
            line(&mut maker, 0, false, uv(0.0, r))?,
        ];
        maker.face(flat(a2, a0)?, vec![coedges], tolerance, !handed, "across1")?;
    }
    maker.body(BodyKind::Solid)
}

/// A closed outline that changes along a sweep: every side a curve of
/// its own degree and knots, and its control points splines along the
/// sweep.
#[derive(Debug, Clone)]
pub struct Loft {
    /// Of each side, its degree and knots.
    pub sides: Vec<(usize, Vec<f64>)>,
    /// How many control points each side has. A side ends where the
    /// next starts.
    pub counts: Vec<usize>,
    /// The degree and the knots along the sweep.
    pub degree: usize,
    /// The knots along the sweep.
    pub knots: Vec<f64>,
    /// The control sections along the sweep: of each, the control
    /// points of all its sides one after the other, in weighted
    /// coordinates (w·x, w·y, w·z and w).
    pub rings: Vec<Vec<[f64; 4]>>,
    /// Whether the last section is the first again: a sweep all the
    /// way around.
    pub closed: bool,
    /// How far the surfaces may be from what they stand for.
    pub tolerance: f64,
}

fn point_of(h: [f64; 4]) -> (Point3, f64) {
    (Point3::new(h[0], h[1], h[2]) / h[3], h[3])
}

/// The basis functions of a spline of `degree` over `knots` that are
/// not zero at `t`: the first one's index, and their values.
pub fn basis(degree: usize, knots: &[f64], t: f64) -> (usize, Vec<f64>) {
    let n = knots.len() - degree - 1;
    let mut span = degree;
    while span + 1 < n && knots[span + 1] <= t {
        span += 1;
    }
    let mut values = vec![0.0; degree + 1];
    values[0] = 1.0;
    let (mut left, mut right) = (vec![0.0; degree + 1], vec![0.0; degree + 1]);
    for j in 1..=degree {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let part = values[r] / (right[r + 1] + left[j - r]);
            values[r] = saved + right[r + 1] * part;
            saved = left[j - r] * part;
        }
        values[j] = saved;
    }
    (span - degree, values)
}

/// The control sections of the sweep that is `wanted[k]` where its
/// basis functions have the values `matrix[k]`. `None` if those do not
/// say.
pub fn through(
    mut matrix: Vec<Vec<f64>>,
    mut wanted: Vec<Vec<[f64; 4]>>,
) -> Option<Vec<Vec<[f64; 4]>>> {
    let n = matrix.len();
    for c in 0..n {
        let pivot = (c..n).max_by(|a, b| matrix[*a][c].abs().total_cmp(&matrix[*b][c].abs()))?;
        if matrix[pivot][c].abs() < 1e-12 {
            return None;
        }
        matrix.swap(c, pivot);
        wanted.swap(c, pivot);
        let top = matrix[c].clone();
        let (above, below) = wanted.split_at_mut(c + 1);
        for (row, section) in matrix[c + 1..].iter_mut().zip(below) {
            let m = row[c] / top[c];
            if m == 0.0 {
                continue;
            }
            for (under, over) in row.iter_mut().zip(&top).skip(c) {
                *under -= m * over;
            }
            for (x, y) in section.iter_mut().zip(&above[c]) {
                for i in 0..4 {
                    x[i] -= m * y[i];
                }
            }
        }
    }
    for c in (0..n).rev() {
        let (upper, lower) = wanted.split_at_mut(c + 1);
        let row = &mut upper[c];
        for (k, known) in lower.iter().enumerate() {
            let m = matrix[c][c + 1 + k];
            if m == 0.0 {
                continue;
            }
            for (x, y) in row.iter_mut().zip(known) {
                for i in 0..4 {
                    x[i] -= m * y[i];
                }
            }
        }
        for x in row.iter_mut() {
            for v in x.iter_mut() {
                *v /= matrix[c][c];
            }
        }
    }
    Some(wanted)
}

/// The sections `flat` gives along `curve` from `range.0` to `range.1`
/// (each the control points of all its sides, x, y, z and weight), as
/// splines along the curve: the degree and knots along it, and the
/// control sections in weighted coordinates, as a [`Loft`] takes them.
///
/// They are splines over the curve's own knots, of its degree or the
/// third: what is swept along a line or a circle between flat and round
/// faces is met exactly, and anything else to within `tolerance`, with
/// more knots. (In weighted coordinates scaled as the curve's own are:
/// where it is a polynomial if the curve is.)
#[allow(clippy::type_complexity)]
pub fn along(
    curve: &Curve3,
    range: (f64, f64),
    tolerance: f64,
    size: f64,
    flat: &mut dyn FnMut(f64) -> anyhow::Result<Vec<[f64; 4]>>,
) -> anyhow::Result<(usize, Vec<f64>, Vec<Vec<[f64; 4]>>)> {
    let degree = curve.degree().max(3);
    let swept = along_of(curve, range, tolerance, size, flat, degree, 16)?;
    swept.context("the sections could not be swept")
}

/// As [`along`], of the curve's own degree, if that is all the sweep
/// is: along a line what does not change is swept straight, and around
/// a circle what turns with it is swept in the circle's own pieces — a
/// surface no finer than what it is of. `None` if it is more than that
/// (then [`along`] sweeps it).
#[allow(clippy::type_complexity)]
pub fn along_plain(
    curve: &Curve3,
    range: (f64, f64),
    tolerance: f64,
    size: f64,
    flat: &mut dyn FnMut(f64) -> anyhow::Result<Vec<[f64; 4]>>,
) -> anyhow::Result<Option<(usize, Vec<f64>, Vec<Vec<[f64; 4]>>)>> {
    let own = curve.degree();
    if own >= 3 {
        return Ok(None);
    }
    along_of(curve, range, tolerance, size, flat, own, 1)
}

/// [`along`], as splines of `degree` (the curve's own, or more), with
/// more knots where it takes them to be within `tolerance`, `passes`
/// times at most: `None` if it is not within that after the last and
/// there was to be no more than one (that degree does not do).
#[allow(clippy::type_complexity)]
fn along_of(
    curve: &Curve3,
    range: (f64, f64),
    tolerance: f64,
    size: f64,
    flat: &mut dyn FnMut(f64) -> anyhow::Result<Vec<[f64; 4]>>,
    degree: usize,
    passes: usize,
) -> anyhow::Result<Option<(usize, Vec<f64>, Vec<Vec<[f64; 4]>>)>> {
    let (from, to) = range;
    let own = curve.degree();
    let slack = (to - from) * 1e-9;
    let mut knots = vec![from; degree + 1];
    let theirs = curve.knots();
    let mut k = 0;
    while k < theirs.len() {
        let same = theirs[k..].iter().take_while(|v| **v == theirs[k]).count();
        if theirs[k] > from + slack && theirs[k] < to - slack {
            let times = (same.min(own) + degree - own).min(degree);
            knots.extend(std::iter::repeat_n(theirs[k], times));
        }
        k += same;
    }
    knots.extend(std::iter::repeat_n(to, degree + 1));
    let scaled = if curve.is_rational() {
        let weights = curve.weights().iter().map(|w| Point3::new(*w, 0.0, 0.0));
        Some(Curve3::new(own, theirs.to_vec(), weights.collect(), None)?)
    } else {
        None
    };
    let scale = |t: f64| scaled.as_ref().map_or(1.0, |c| c.point_at(t).x);
    let mut rings: Vec<Vec<[f64; 4]>> = Vec::new();
    for pass in 0..passes {
        let n = knots.len() - degree - 1;
        // Through the sections at as many places as it has control
        // points: where each of those weighs most.
        let mut matrix = vec![vec![0.0; n]; n];
        let mut wanted: Vec<Vec<[f64; 4]>> = Vec::new();
        for (j, row) in matrix.iter_mut().enumerate() {
            // (Never past the ends, as the mean of knots all at an end
            // can come to by a hair: a section is asked for along the
            // stretch alone.)
            let t = (knots[j + 1..=j + degree].iter().sum::<f64>() / degree as f64).clamp(from, to);
            let (first, values) = basis(degree, &knots, t);
            row[first..first + values.len()].copy_from_slice(&values);
            let by = scale(t);
            let weigh = |h: &[f64; 4]| {
                [
                    h[0] * h[3] * by,
                    h[1] * h[3] * by,
                    h[2] * h[3] * by,
                    h[3] * by,
                ]
            };
            wanted.push(flat(t)?.iter().map(weigh).collect());
        }
        rings = through(matrix, wanted).context("the sections could not be swept")?;
        // Is it the sweep between those places too?
        let mut more = Vec::new();
        for span in degree..n {
            let (a, b) = (knots[span], knots[span + 1]);
            if b <= a {
                continue;
            }
            let mut worst = 0.0_f64;
            for f in [0.21, 0.5, 0.79] {
                let t = a + (b - a) * f;
                let truth = flat(t)?;
                let (first, values) = basis(degree, &knots, t);
                for (i, h) in truth.iter().enumerate() {
                    let mut swept = [0.0; 4];
                    for (value, ring) in values.iter().zip(&rings[first..]) {
                        for c in 0..4 {
                            swept[c] += value * ring[i][c];
                        }
                    }
                    let at = Point3::new(swept[0], swept[1], swept[2]) / swept[3];
                    let apart = Point3::new(h[0], h[1], h[2]).distance(at)
                        + (h[3] - swept[3] / scale(t)).abs() * size * 1e-3;
                    worst = worst.max(apart);
                }
            }
            if worst > tolerance {
                more.push((a + b) / 2.0);
            }
        }
        if more.is_empty() || n > 400 {
            break;
        }
        if passes == 1 {
            return Ok(None);
        }
        // (After the last pass, the knots stay those the sections are
        // of.)
        if pass + 1 == passes {
            break;
        }
        knots.extend(more);
        knots.sort_by(f64::total_cmp);
    }
    Ok(Some((degree, knots, rings)))
}

/// Sweeps the outline of `loft` through its sections into the solid
/// body `id`: a face along each side, and a flat one across each end
/// (the sections at the ends have to be flat).
pub fn loft(topo: &mut Topo, id: &str, loft: &Loft) -> anyhow::Result<BodyId> {
    let rings = &loft.rings;
    let (sides, n) = (loft.sides.len(), rings.len());
    anyhow::ensure!(sides >= 3 && n >= 2, "nothing to sweep through");
    let along = &loft.knots;
    let (v0, v1) = (along[0], along[along.len() - 1]);
    let tolerance = loft.tolerance;
    let mut maker = Maker::new(topo, id)?;
    // Where each side's control points start in a section.
    let starts: Vec<usize> = loft
        .counts
        .iter()
        .scan(0, |at, count| {
            *at += count;
            Some(*at - count)
        })
        .collect();
    let side = |ring: &[[f64; 4]], k: usize| -> Vec<[f64; 4]> {
        ring[starts[k]..starts[k] + loft.counts[k]].to_vec()
    };
    let corner = |ring: &[[f64; 4]], k: usize| point_of(ring[starts[k]]).0;
    // The corners at the two ends (one end, all the way around), and
    // the curve each corner runs along.
    // A corner that stays where it is all along the sweep is one
    // point: the faces to its sides close in it, and a side between two
    // of them sweeps no face at all.
    let rests = |i: usize| {
        let at = point_of(rings[0][i]).0;
        rings
            .iter()
            .all(|ring| point_of(ring[i]).0.distance(at) <= tolerance)
    };
    let still: Vec<bool> = (0..sides).map(|k| rests(starts[k])).collect();
    let gone: Vec<bool> = (0..sides)
        .map(|k| (starts[k]..starts[k] + loft.counts[k]).all(&rests) && still[(k + 1) % sides])
        .collect();
    // A side that is one point at an end (a square's corner that
    // opens into a quarter of a circle, a spire's top) has no edge
    // there: its face closes in that point, which the corners to its
    // two sides share.
    let shut = |ring: &[[f64; 4]], k: usize| {
        let at = corner(ring, k);
        let all = (starts[k]..starts[k] + loft.counts[k])
            .all(|i| point_of(ring[i]).0.distance(at) <= tolerance);
        all && corner(ring, (k + 1) % sides).distance(at) <= tolerance
    };
    let shut: [Vec<bool>; 2] = [
        (0..sides).map(|k| shut(&rings[0], k)).collect(),
        (0..sides).map(|k| shut(&rings[n - 1], k)).collect(),
    ];
    let mut ends: [Vec<VertexId>; 2] = [Vec::new(), Vec::new()];
    for (k, &stays) in still.iter().enumerate() {
        // (The corner before, if the side from it is shut here; and,
        // for the last corner, the first if the side on to it is.)
        let shared = |maker: &mut Maker, end: usize, made: &[VertexId]| {
            let ring = &rings[if end == 0 { 0 } else { n - 1 }];
            if k > 0 && shut[end][k - 1] {
                return Ok(made[k - 1]);
            }
            if k == sides - 1 && shut[end][k] {
                return Ok(made[0]);
            }
            let name = if end == 0 { "start" } else { "end" };
            maker.vertex(corner(ring, k), tolerance * 4.0, &format!("{name}/v{k}"))
        };
        let first = shared(&mut maker, 0, &ends[0])?;
        let last = if loft.closed || stays {
            first
        } else {
            shared(&mut maker, 1, &ends[1])?
        };
        ends[0].push(first);
        ends[1].push(last);
    }
    let mut rails = Vec::new();
    for k in 0..sides {
        if still[k] {
            rails.push(None);
            continue;
        }
        let (points, weights): (Vec<Point3>, Vec<f64>) =
            rings.iter().map(|ring| point_of(ring[starts[k]])).unzip();
        let curve = Curve3::new(loft.degree, along.clone(), points, Some(weights))?;
        rails.push(Some(maker.edge(
            curve,
            None,
            (ends[0][k], ends[1][k]),
            tolerance,
            &format!("along/v{k}"),
        )?));
    }
    // The sides of the sections at the two ends.
    let section = |ring: &[[f64; 4]], k: usize| {
        let (degree, knots) = &loft.sides[k];
        let (points, weights): (Vec<Point3>, Vec<f64>) =
            side(ring, k).into_iter().map(point_of).unzip();
        Curve3::new(*degree, knots.clone(), points, Some(weights))
    };
    let mut across = [Vec::new(), Vec::new()];
    for k in 0..sides {
        let next = (k + 1) % sides;
        let first = if shut[0][k] && !gone[k] {
            None
        } else {
            Some(maker.edge(
                section(&rings[0], k)?,
                None,
                (ends[0][k], ends[0][next]),
                tolerance,
                &format!("start/e{k}"),
            )?)
        };
        let last = if loft.closed || gone[k] {
            first
        } else if shut[1][k] {
            None
        } else {
            Some(maker.edge(
                section(&rings[n - 1], k)?,
                None,
                (ends[1][k], ends[1][next]),
                tolerance,
                &format!("end/e{k}"),
            )?)
        };
        across[0].push(first);
        across[1].push(last);
    }
    // Where the outline is, about: to tell which way is out of it.
    let middle =
        |ring: &[[f64; 4]]| (0..sides).map(|k| corner(ring, k)).sum::<Point3>() / sides as f64;
    // Every side runs around the outline the same way, so they all
    // face out of it or all into it: out when the outline turns
    // counter-clockwise around the way the sweep goes. (A section may
    // well bend inwards, so where one side lies says nothing.)
    let around = |ring: &[[f64; 4]]| -> Point3 {
        let origin = corner(ring, 0);
        (0..sides)
            .map(|k| (corner(ring, k) - origin).cross(corner(ring, (k + 1) % sides) - origin))
            .sum()
    };
    let mid = (n / 2).min(n - 2);
    let onward = middle(&rings[mid + 1]) - middle(&rings[mid]);
    let turns = around(&rings[mid]).dot(onward) + around(&rings[mid + 1]).dot(onward);
    let outward = turns > 0.0;
    let uv = Point2::new;
    for k in 0..sides {
        if gone[k] {
            continue;
        }
        let (degree, knots) = &loft.sides[k];
        let count = loft.counts[k];
        let mut points = Vec::new();
        let mut weights = Vec::new();
        for i in 0..count {
            for ring in rings {
                let (p, w) = point_of(ring[starts[k] + i]);
                points.push(p);
                weights.push(w);
            }
        }
        let surface = NurbsSurface::new(
            *degree,
            loft.degree,
            knots.clone(),
            along.clone(),
            count,
            n,
            points,
            Some(weights),
        )?;
        let ((u0, u1), _) = surface.domain();
        let next = (k + 1) % sides;
        let coedges = vec![
            match across[0][k] {
                Some(edge) => maker.coedge(edge, true, uv(u0, v0), uv(u1, v0))?,
                None => maker.pole(ends[0][k], uv(u0, v0), uv(u1, v0))?,
            },
            match rails[next] {
                Some(rail) => maker.coedge(rail, true, uv(u1, v0), uv(u1, v1))?,
                None => maker.pole(ends[0][next], uv(u1, v0), uv(u1, v1))?,
            },
            match across[1][k] {
                Some(edge) => maker.coedge(edge, false, uv(u0, v1), uv(u1, v1))?,
                None => maker.pole(ends[1][k], uv(u1, v1), uv(u0, v1))?,
            },
            match rails[k] {
                Some(rail) => maker.coedge(rail, false, uv(u0, v0), uv(u0, v1))?,
                None => maker.pole(ends[0][k], uv(u0, v1), uv(u0, v0))?,
            },
        ];
        maker.face(
            surface,
            vec![coedges],
            tolerance,
            outward,
            &format!("side/e{k}"),
        )?;
    }
    if !loft.closed {
        // The flat ends: each the outline of its section, in its plane.
        for (end, name) in [(0, "start"), (1, "end")] {
            // (An end that closes to a point has no face.)
            if across[end].iter().all(Option::is_none) {
                continue;
            }
            let ring = &rings[if end == 0 { 0 } else { n - 1 }];
            let corners: Vec<Point3> = (0..sides).map(|k| corner(ring, k)).collect();
            let origin = corners[0];
            // The plane's normal, from the outline itself.
            let normal: Point3 = (0..sides)
                .map(|k| (corners[k] - origin).cross(corners[(k + 1) % sides] - origin))
                .sum();
            let normal = normal.try_normalize().context("a section of no area")?;
            // The plane faces out of the solid: away from the other
            // sections. Its outline is walked counter-clockwise around
            // that: the sides' own way, or against it.
            let inward = middle(&rings[if end == 0 { 1 } else { n - 2 }]) - middle(ring);
            let forward = normal.dot(inward) < 0.0;
            let normal = if forward { normal } else { -normal };
            let x = corners
                .iter()
                .find_map(|c| (*c - origin).reject_from(normal).try_normalize())
                .context("a section of no width")?;
            let y = normal.cross(x);
            let flat = |p: Point3| Point2::new((p - origin).dot(x), (p - origin).dot(y));
            let all: Vec<Point2> = ring.iter().map(|h| flat(point_of(*h).0)).collect();
            let (mut lo, mut hi) = (Point2::INFINITY, Point2::NEG_INFINITY);
            for p in &all {
                (lo, hi) = (lo.min(*p), hi.max(*p));
            }
            let pad = (hi - lo).length() * 0.1 + tolerance;
            let (lo, hi) = (lo - Point2::splat(pad), hi + Point2::splat(pad));
            let place = |p: Point2| origin + x * p.x + y * p.y;
            let surface = NurbsSurface::new(
                1,
                1,
                vec![lo.x, lo.x, hi.x, hi.x],
                vec![lo.y, lo.y, hi.y, hi.y],
                2,
                2,
                vec![
                    place(lo),
                    place(Point2::new(lo.x, hi.y)),
                    place(Point2::new(hi.x, lo.y)),
                    place(hi),
                ],
                None,
            )?;
            let mut coedges = Vec::new();
            for (k, &edge) in across[end].iter().enumerate() {
                let Some(edge) = edge else {
                    continue;
                };
                let (degree, knots) = &loft.sides[k];
                let (points, weights): (Vec<Point2>, Vec<f64>) = side(ring, k)
                    .into_iter()
                    .map(|h| (flat(point_of(h).0), h[3]))
                    .unzip();
                let pcurve = Curve2::new(*degree, knots.clone(), points, Some(weights))?;
                coedges.push(maker.coedge_on(edge, forward, pcurve)?);
            }
            if !forward {
                coedges.reverse();
            }
            maker.face(surface, vec![coedges], tolerance, true, name)?;
        }
    }
    maker.body(BodyKind::Solid)
}

#[cfg(test)]
mod even_tests {
    use super::*;

    #[test]
    fn a_corner_is_the_same_from_its_three_sides() {
        let r = 2.0;
        let w = crate::gn::even(std::f64::consts::FRAC_PI_2);
        let p = Point3::new;
        let points = crate::gn::section(p(0.0, r, 0.0), p(r, r, 0.0), p(r, 0.0, 0.0), w);
        let section = Curve2::new(
            5,
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            points.iter().map(|p| Point2::new(p[0], p[1])).collect(),
            Some(points.iter().map(|p| p[3]).collect()),
        )
        .unwrap();
        let axes = [Point3::X, Point3::Y, Point3::Z];
        let even = even_corner(Point3::ZERO, axes, &section).expect("the same from both ends");
        // It comes to a point on the third axis, and ends in the
        // section; its middle is as far along all three.
        assert!(even(0.4, 0.0).distance(p(0.0, 0.0, r)) < 1e-12);
        let end = section.point_at(0.3);
        assert!(even(0.3, 1.0).distance(p(end.y, end.x, 0.0)) < 1e-12);
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..60 {
            let t = (lo + hi) / 2.0;
            let at = even(0.5, t);
            if at.z > at.x { lo = t } else { hi = t }
        }
        let middle = even(0.5, lo);
        assert!((middle.x - middle.y).abs() < 1e-12 && (middle.x - middle.z).abs() < 1e-9);
        // Any point of it, with its axes swapped, is a point of it.
        let at = even(0.3, 0.6);
        let swapped = p(at.z, at.x, at.y);
        let mut near = f64::INFINITY;
        for i in 0..=400 {
            for j in 0..=400 {
                near = near.min(even(f64::from(i) / 400.0, f64::from(j) / 400.0).distance(swapped));
            }
        }
        assert!(near < 0.01, "{near}");
    }
}
