//! A solid as a boolean sees it: vertices, edges on NURBS curves and
//! faces on NURBS surfaces, every one with its tolerance, and each use of
//! an edge by a face with its curve in the surface's (u, v).

use std::sync::Arc;

use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{Curve2, Curve3, NurbsError, NurbsSurface};

use crate::bezier::Bounds;

/// A vertex.
#[derive(Debug, Clone)]
pub struct Vertex {
    /// Where it is.
    pub point: Point3,
    /// The radius of the ball the true vertex lies in.
    pub tolerance: f64,
    /// Its name.
    pub name: Option<String>,
}

/// An edge: a piece of a curve between two vertices.
#[derive(Debug, Clone)]
pub struct Edge {
    /// The curve.
    pub curve: Arc<Curve3>,
    /// The parameters of the piece, the first less than the second.
    pub range: (f64, f64),
    /// The vertices at its ends (the same one twice for a closed edge).
    pub ends: [usize; 2],
    /// The radius of the tube the true edge lies in.
    pub tolerance: f64,
    /// Its name.
    pub name: Option<String>,
    /// The names it is known by besides: those of the edges that were
    /// made one with it.
    pub also: Vec<String>,
}

impl Edge {
    /// The point at parameter `t`.
    pub fn point(&self, t: f64) -> Point3 {
        self.curve.point_at(t)
    }

    /// The parameter in the middle of the piece.
    pub fn middle(&self) -> f64 {
        (self.range.0 + self.range.1) / 2.0
    }
}

/// What a loop walks along.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Walk {
    /// An edge, along its curve or against it.
    Edge {
        /// The edge.
        edge: usize,
        /// Whether it is walked against its curve.
        reversed: bool,
    },
    /// No edge: the loop stays at a vertex where the surface collapses
    /// (a sphere's pole, a cone's apex) while its (u, v) curve runs along
    /// the collapsed side.
    Pole {
        /// The vertex.
        vertex: usize,
        /// The parameters of the (u, v) curve, walked upwards.
        range: (f64, f64),
    },
}

/// One step of a loop: what is walked, and its curve in the face's
/// (u, v). An edge's curve there has the edge's parameters and runs the
/// edge's way, whichever way the loop walks it.
#[derive(Debug, Clone)]
pub struct Use {
    /// What is walked.
    pub walk: Walk,
    /// Its curve in (u, v).
    pub pcurve: Arc<Curve2>,
}

/// A face: a region of a surface bounded by loops.
#[derive(Debug, Clone)]
pub struct Face {
    /// The surface.
    pub surface: Arc<NurbsSurface>,
    /// Whether the outside of the solid is where the surface's normal
    /// `∂u × ∂v` points.
    pub outward: bool,
    /// The loops. Walking one with the surface's normal up, the face is
    /// on the left.
    pub loops: Vec<Vec<Use>>,
    /// The half-thickness of the slab the true face lies in.
    pub tolerance: f64,
    /// Its name.
    pub name: Option<String>,
    /// The names it is known by besides: those of the faces that were
    /// made one with it.
    pub also: Vec<String>,
    /// Those of its edges (by index) along which it runs on smoothly
    /// into whatever face of another solid the edge lies in: what the
    /// maker of a blend knows of it. Such two faces touch along the
    /// edge and are not looked at for where else they might cross —
    /// which, for two faces as close as they are told apart, would be
    /// anywhere.
    pub smooth: Vec<usize>,
}

/// A solid: closed shells of faces.
#[derive(Debug, Clone, Default)]
pub struct Solid {
    /// The vertices.
    pub vertices: Vec<Vertex>,
    /// The edges.
    pub edges: Vec<Edge>,
    /// The faces.
    pub faces: Vec<Face>,
}

impl Solid {
    /// Whether there is nothing to it.
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// The parameters `step` is walked over, in walking order: where its
    /// (u, v) curve starts and ends.
    pub fn span(&self, step: &Use) -> (f64, f64) {
        match step.walk {
            Walk::Edge { edge, reversed } => {
                let (a, b) = self.edges[edge].range;
                if reversed { (b, a) } else { (a, b) }
            }
            Walk::Pole { range, .. } => range,
        }
    }

    /// The vertices `step` starts and ends at.
    pub fn ends(&self, step: &Use) -> (usize, usize) {
        match step.walk {
            Walk::Edge { edge, reversed } => {
                let [a, b] = self.edges[edge].ends;
                if reversed { (b, a) } else { (a, b) }
            }
            Walk::Pole { vertex, .. } => (vertex, vertex),
        }
    }

    /// Where in (u, v) `step` starts.
    pub fn start(&self, step: &Use) -> Point2 {
        step.pcurve.point_at(self.span(step).0)
    }

    /// The corners of a box around the solid: around its faces'
    /// surfaces as far as their loops reach, so a little more than the
    /// solid itself.
    pub fn extent(&self) -> (Point3, Point3) {
        let mut b = Bounds::EMPTY;
        for face in &self.faces {
            b = b.union(crate::query::FaceData::new(self, face).bounds);
        }
        (b.lo, b.hi)
    }

    /// The largest tolerance of anything in it.
    pub fn tolerance(&self) -> f64 {
        let vertices = self.vertices.iter().map(|v| v.tolerance);
        let edges = self.edges.iter().map(|e| e.tolerance);
        let faces = self.faces.iter().map(|f| f.tolerance);
        vertices.chain(edges).chain(faces).fold(0.0, f64::max)
    }

    /// The tolerances the vertices and edges need to hold what lies on
    /// them: an edge its curves in the faces' (u, v), carried to space,
    /// and its faces' tolerances; a vertex the ends of its edges. (The
    /// vertices' first, then the edges'.)
    pub fn tolerances(&self) -> (Vec<f64>, Vec<f64>) {
        let solid = self;
        let grow = |t: f64| t * (1.0 + 1e-9) + 1e-13;
        let mut edges: Vec<f64> = solid.edges.iter().map(|e| e.tolerance).collect();
        let mut vertices: Vec<f64> = solid.vertices.iter().map(|v| v.tolerance).collect();
        for face in &solid.faces {
            // A flat face laid out evenly: its point goes evenly with
            // both parameters.
            let even = {
                let s = &face.surface;
                let p = s.control_points();
                s.degrees() == (1, 1)
                    && p.len() == 4
                    && s.weights().iter().all(|w| *w == 1.0)
                    && (p[3] - p[2] - p[1] + p[0]).length() <= 1e-13 * p[0].distance(p[3])
            };
            for step in face.loops.iter().flatten() {
                let (a, b) = solid.span(step);
                let on_surface = |t: f64| {
                    let uv = step.pcurve.point_at(t);
                    face.surface.point_at(uv.x, uv.y)
                };
                let at = [0.0, 0.25, 0.5, 0.75, 1.0].map(|f| a + (b - a) * f);
                match step.walk {
                    Walk::Edge { edge, .. } => {
                        let e = &solid.edges[edge];
                        // A straight edge, straight across such a
                        // face: the two go evenly with the edge's
                        // parameter, and are farthest apart at an end.
                        let plain = |degree: usize, points: usize, weights: &[f64]| {
                            degree == 1 && points == 2 && weights.iter().all(|w| *w == 1.0)
                        };
                        if even
                            && plain(
                                e.curve.degree(),
                                e.curve.control_points().len(),
                                e.curve.weights(),
                            )
                            && plain(
                                step.pcurve.degree(),
                                step.pcurve.control_points().len(),
                                step.pcurve.weights(),
                            )
                        {
                            let off = [a, b]
                                .map(|t| on_surface(t).distance(e.point(t)))
                                .into_iter()
                                .fold(0.0, f64::max);
                            edges[edge] = edges[edge].max(face.tolerance).max(grow(off));
                            continue;
                        }
                        // Span by span of the two curves: a curve of
                        // many spans strays between a few points only.
                        let mut stops = at.to_vec();
                        let inner = |k: &f64| *k > a.min(b) && *k < a.max(b);
                        stops.extend(e.curve.knots().iter().copied().filter(inner));
                        stops.extend(step.pcurve.knots().iter().copied().filter(inner));
                        stops.sort_by(f64::total_cmp);
                        stops.dedup();
                        let at = stops.windows(2).flat_map(|w| {
                            [0.0, 0.25, 0.5, 0.75, 1.0].map(|f| w[0] + (w[1] - w[0]) * f)
                        });
                        let off = at
                            .map(|t| on_surface(t).distance(e.point(t)))
                            .fold(0.0, f64::max);
                        edges[edge] = edges[edge].max(face.tolerance).max(grow(off));
                    }
                    Walk::Pole { vertex, .. } => {
                        let p = solid.vertices[vertex].point;
                        let off = at
                            .into_iter()
                            .map(|t| on_surface(t).distance(p))
                            .fold(0.0, f64::max);
                        vertices[vertex] = vertices[vertex].max(grow(off));
                    }
                }
            }
        }
        for (k, e) in solid.edges.iter().enumerate() {
            for (end, t) in [(e.ends[0], e.range.0), (e.ends[1], e.range.1)] {
                let off = e.point(t).distance(solid.vertices[end].point);
                vertices[end] = vertices[end].max(grow(off + edges[k]));
            }
        }
        (vertices, edges)
    }

    /// Gives every vertex and edge the tolerance it needs (see
    /// [`tolerances`](Self::tolerances)).
    pub fn settle(&mut self) {
        let (vertices, edges) = self.tolerances();
        for (v, t) in self.vertices.iter_mut().zip(vertices) {
            v.tolerance = t;
        }
        for (e, t) in self.edges.iter_mut().zip(edges) {
            e.tolerance = t;
        }
    }

    /// The solid moved: every point of it taken to `to` of it, which
    /// has to keep lines straight and lengths as they are (a turn and a
    /// shift), so that the control points moved are the curves and
    /// surfaces moved, and the tolerances stay.
    pub fn moved(&self, to: &dyn Fn(Point3) -> Point3) -> Result<Solid, NurbsError> {
        self.mapped(to, false, 1.0)
    }

    /// The solid with every point of it taken to `to` of it: a turn and
    /// a shift as for [`Solid::moved`], and with them a reflection (if
    /// `mirrored`: inside and outside then change sides of every
    /// surface) and a growing or shrinking by `scale` about some point
    /// (the tolerances grow or shrink with it).
    pub fn mapped(
        &self,
        to: &dyn Fn(Point3) -> Point3,
        mirrored: bool,
        scale: f64,
    ) -> Result<Solid, NurbsError> {
        let mut out = self.clone();
        for v in &mut out.vertices {
            v.tolerance *= scale;
        }
        for e in &mut out.edges {
            e.tolerance *= scale;
        }
        for f in &mut out.faces {
            f.tolerance *= scale;
            f.outward ^= mirrored;
        }
        for v in &mut out.vertices {
            v.point = to(v.point);
        }
        // Curves and surfaces that are shared stay shared.
        let mut curves: Vec<(*const Curve3, Arc<Curve3>)> = Vec::new();
        for e in &mut out.edges {
            let key = Arc::as_ptr(&e.curve);
            let moved = match curves.iter().find(|c| c.0 == key) {
                Some(c) => c.1.clone(),
                None => {
                    let c = &e.curve;
                    let moved = Arc::new(Curve3::new(
                        c.degree(),
                        c.knots().to_vec(),
                        c.control_points().iter().map(|p| to(*p)).collect(),
                        Some(c.weights().to_vec()),
                    )?);
                    curves.push((key, moved.clone()));
                    moved
                }
            };
            e.curve = moved;
        }
        let mut surfaces: Vec<(*const NurbsSurface, Arc<NurbsSurface>)> = Vec::new();
        for f in &mut out.faces {
            let key = Arc::as_ptr(&f.surface);
            let moved = match surfaces.iter().find(|s| s.0 == key) {
                Some(s) => s.1.clone(),
                None => {
                    let s = &f.surface;
                    let (du, dv) = s.degrees();
                    let (ku, kv) = s.knots();
                    let (nu, nv) = s.counts();
                    let moved = Arc::new(NurbsSurface::new(
                        du,
                        dv,
                        ku.to_vec(),
                        kv.to_vec(),
                        nu,
                        nv,
                        s.control_points().iter().map(|p| to(*p)).collect(),
                        Some(s.weights().to_vec()),
                    )?);
                    surfaces.push((key, moved.clone()));
                    moved
                }
            };
            f.surface = moved;
        }
        Ok(out)
    }

    /// The solid under any map `to` that keeps lines straight and
    /// parallels parallel (a stretch along axes, a shear, with a turn,
    /// a shift or a reflection): the control points mapped are the
    /// curves and surfaces mapped, exactly. `mirrored` says that the
    /// map turns space inside out (its determinant is less than
    /// nothing), and `most` is the most it stretches any length: what
    /// the tolerances grow by.
    pub fn sheared(
        &self,
        to: &dyn Fn(Point3) -> Point3,
        mirrored: bool,
        most: f64,
    ) -> Result<Solid, NurbsError> {
        self.mapped(to, mirrored, most)
    }

    /// Gives each pair of faces that bounds one stretch of material
    /// around an edge four faces come to an edge of its own: two solids
    /// that touch along an edge, written as one, share that edge four
    /// ways, and a solid's edges are shared by two faces. Says how many
    /// edges were parted; an edge whose faces do not pair up that way is
    /// left as it is.
    pub fn part_edges(&mut self) -> usize {
        // Every use of every edge: the face, its loop, the step.
        let mut uses: Vec<Vec<[usize; 3]>> = vec![Vec::new(); self.edges.len()];
        for (f, face) in self.faces.iter().enumerate() {
            for (l, steps) in face.loops.iter().enumerate() {
                for (s, step) in steps.iter().enumerate() {
                    if let Walk::Edge { edge, .. } = step.walk {
                        uses[edge].push([f, l, s]);
                    }
                }
            }
        }
        let mut parted = 0;
        for (e, uses) in uses.iter().enumerate() {
            if uses.len() != 4 {
                continue;
            }
            let edge = &self.edges[e];
            let t = edge.middle();
            let (at, along) = edge.curve.derivative_at(t);
            let Some(along) = along.try_normalize() else {
                continue;
            };
            let e1 = along.any_orthonormal_vector();
            let e2 = along.cross(e1);
            let reach = self
                .faces
                .iter()
                .flat_map(|f| f.surface.control_points())
                .fold(0.0_f64, |m, p| m.max(p.distance(at)))
                * 1e-3;
            // Of each use: the angle its face leaves the edge at, whether
            // its material is on from there the way the angle rises,
            // and which way it walks the edge.
            let mut around: Vec<(f64, bool, bool, [usize; 3])> = Vec::new();
            for &[f, l, s] in uses {
                let face = &self.faces[f];
                let step = &face.loops[l][s];
                let Walk::Edge { reversed, .. } = step.walk else {
                    continue;
                };
                let (uv, d) = step.pcurve.derivative_at(t);
                let walk = if reversed { -d } else { d };
                let into = Point2::new(-walk.y, walk.x);
                let (p, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
                let size = (du * into.x + dv * into.y).length().max(1e-300);
                let q = uv + into * (reach / size);
                let leaving = face.surface.point_at(q.x, q.y) - p;
                let out = du.cross(dv) * if face.outward { 1.0 } else { -1.0 };
                around.push((
                    leaving.dot(e2).atan2(leaving.dot(e1)),
                    -out.dot(along.cross(leaving)) > 0.0,
                    reversed == face.outward,
                    [f, l, s],
                ));
            }
            if around.len() != 4 {
                continue;
            }
            around.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut pairs: Vec<[[usize; 3]; 2]> = Vec::new();
            let mut fits = true;
            for i in 0..4 {
                let (a, b) = (around[i], around[(i + 1) % 4]);
                if a.1 {
                    fits &= !b.1 && a.2 != b.2;
                    pairs.push([a.3, b.3]);
                }
            }
            if !fits || pairs.len() != 2 {
                continue;
            }
            // The second pair walks a copy of the edge.
            let copy = self.edges.len();
            self.edges.push(self.edges[e].clone());
            for [f, l, s] in pairs[1] {
                if let Walk::Edge { edge, .. } = &mut self.faces[f].loops[l][s].walk {
                    *edge = copy;
                }
            }
            parted += 1;
        }
        parted
    }

    /// The faces in groups that hang together through edges.
    pub fn shells(&self) -> Vec<Vec<usize>> {
        let mut of_edge: Vec<Vec<usize>> = vec![Vec::new(); self.edges.len()];
        for (f, face) in self.faces.iter().enumerate() {
            for step in face.loops.iter().flatten() {
                if let Walk::Edge { edge, .. } = step.walk {
                    of_edge[edge].push(f);
                }
            }
        }
        let mut shell = vec![usize::MAX; self.faces.len()];
        let mut shells = Vec::new();
        for start in 0..self.faces.len() {
            if shell[start] != usize::MAX {
                continue;
            }
            let mut faces = vec![start];
            shell[start] = shells.len();
            let mut next = 0;
            while next < faces.len() {
                let f = faces[next];
                next += 1;
                for step in self.faces[f].loops.iter().flatten() {
                    let Walk::Edge { edge, .. } = step.walk else {
                        continue;
                    };
                    for &g in &of_edge[edge] {
                        if shell[g] == usize::MAX {
                            shell[g] = shells.len();
                            faces.push(g);
                        }
                    }
                }
            }
            faces.sort_unstable();
            shells.push(faces);
        }
        shells
    }

    /// The solid of only `faces`, with the vertices and edges they use.
    pub fn part(&self, faces: &[usize]) -> Solid {
        let mut out = Solid::default();
        let mut vertex = vec![usize::MAX; self.vertices.len()];
        let mut edge = vec![usize::MAX; self.edges.len()];
        let mut take_vertex = |out: &mut Solid, v: usize| {
            if vertex[v] == usize::MAX {
                vertex[v] = out.vertices.len();
                out.vertices.push(self.vertices[v].clone());
            }
            vertex[v]
        };
        for &f in faces {
            let mut face = self.faces[f].clone();
            for step in face.loops.iter_mut().flatten() {
                match &mut step.walk {
                    Walk::Edge { edge: e, .. } => {
                        if edge[*e] == usize::MAX {
                            let mut new = self.edges[*e].clone();
                            new.ends = new.ends.map(|v| take_vertex(&mut out, v));
                            edge[*e] = out.edges.len();
                            out.edges.push(new);
                        }
                        *e = edge[*e];
                    }
                    Walk::Pole { vertex: v, .. } => *v = take_vertex(&mut out, *v),
                }
            }
            out.faces.push(face);
        }
        out
    }
}
