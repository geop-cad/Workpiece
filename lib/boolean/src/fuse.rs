//! The general fuse: where two solids meet, their faces cut along it,
//! and the pieces an operation keeps.
//!
//! The work goes up by dimension, so that each step can take what is
//! below it as settled:
//!
//! 1. Vertices on vertices: those whose balls meet are one.
//! 2. Vertices on edges: an edge is cut where a vertex of the other
//!    solid lies on it.
//! 3. Edges on edges: cut where they cross; pieces that are the same
//!    curve between the same vertices become one edge of both solids.
//! 4. Vertices in faces, and 5. edges through faces: an edge is cut
//!    where it pierces a face of the other solid, and one that lies in
//!    a face is drawn on it.
//! 6. Faces through faces: from every vertex that is on a face of each
//!    solid, the curve where the two surfaces cross is followed to the
//!    next such vertex and becomes an edge on both. Closed crossings no
//!    edge reaches are looked for where the surfaces' patches overlap.
//!
//! After that no face straddles the other solid; `cells` cuts each face
//! along what was drawn on it and sorts the pieces.
//!
//! Every "meets" is by the tolerances of the two things meeting, found
//! by the one contact search of `solve`; no curve or surface is treated
//! differently for what it is.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::topo::{Curve2, Curve3, NurbsSurface};

use crate::bezier::{Bounds, Piece, curve_pieces};
use crate::fit::{project, pull, reparametrize};
use crate::model::{Solid, Walk};
use crate::query::{SolidData, normal};
use crate::solve::{Along, At, Over, contacts};
use crate::trace::{Goal, Node, Stop, Tracer};
use crate::{Op, Refusal};

/// What a boolean made.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The solid; empty if nothing is left.
    pub solid: Solid,
    /// Whether the two solids had anything to do with each other: they
    /// cross, touch, or one is inside the other.
    pub touched: bool,
}

/// A vertex of the fuse.
pub(crate) struct Vert {
    pub point: Point3,
    pub tol: f64,
    pub name: Option<String>,
    /// Which of the solids it is a vertex of.
    pub of: [bool; 2],
}

/// An edge of the fuse: a piece of an edge of one of the solids, or a
/// new one where faces cross.
pub(crate) struct Part {
    pub curve: Arc<Curve3>,
    pub range: (f64, f64),
    pub ends: [usize; 2],
    pub tol: f64,
    pub name: Option<String>,
    pub bez: Vec<Piece>,
    pub bounds: Bounds,
    pub alive: bool,
    /// The solid and edge it is a piece of.
    pub owner: Option<(usize, usize)>,
    /// Whether it lies on the boundary of both solids.
    pub shared: bool,
}

impl Part {
    pub(crate) fn middle(&self) -> f64 {
        (self.range.0 + self.range.1) / 2.0
    }
}

/// One piece of an edge of a solid: the part that stands for it now.
#[derive(Clone)]
pub(crate) struct Link {
    pub part: usize,
    /// The piece's parameters on the edge's own curve.
    pub range: (f64, f64),
    /// Whether the part runs against the edge.
    pub reversed: bool,
    /// Whether the part is on another curve than the edge's (it was an
    /// edge of the other solid first).
    pub foreign: bool,
}

/// One walk along a part (or around a pole) on a face, with its curve
/// in the face's (u, v), which has the part's parameters.
#[derive(Clone)]
pub(crate) struct Half {
    pub part: Option<usize>,
    /// Whether it walks the way the part's parameter rises.
    pub forward: bool,
    pub tail: usize,
    pub head: usize,
    pub pcurve: Arc<Curve2>,
    pub range: (f64, f64),
}

impl Half {
    /// The parameter `f` of the way from its tail to its head.
    pub(crate) fn at(&self, f: f64) -> f64 {
        let (a, b) = self.range;
        if self.forward {
            a + (b - a) * f
        } else {
            b + (a - b) * f
        }
    }

    pub(crate) fn uv(&self, f: f64) -> Point2 {
        self.pcurve.point_at(self.at(f))
    }

    /// The direction it leaves its tail in (`from_head`: its head,
    /// backwards).
    pub(crate) fn leaving(&self, from_head: bool, f: f64) -> Point2 {
        let (a, b) = if from_head {
            (self.uv(1.0), self.uv(1.0 - f))
        } else {
            (self.uv(0.0), self.uv(f))
        };
        b - a
    }
}

/// Where a vertex is on a face.
#[derive(Clone, Copy)]
pub(crate) enum Site {
    /// On its boundary, between the walk arriving and the one leaving:
    /// the (u, v), and the directions the boundary leaves it in, onwards
    /// and backwards.
    Corner {
        uv: Point2,
        onward: Point2,
        back: Point2,
        /// Whether the surface collapses here, so that directions in
        /// (u, v) tell nothing.
        pole: bool,
    },
    /// Inside it.
    Inner { uv: Point2 },
}

impl Site {
    pub(crate) fn uv(&self) -> Point2 {
        match *self {
            Site::Corner { uv, .. } | Site::Inner { uv } => uv,
        }
    }

    /// Whether the surface collapses to a point here.
    fn at_pole(&self) -> bool {
        matches!(self, Site::Corner { pole: true, .. })
    }

    /// Whether the (u, v) direction `w` from here goes into the face;
    /// `None` if it runs along the boundary too nearly to tell.
    fn enters(&self, w: Point2) -> Option<bool> {
        let Site::Corner {
            onward, back, pole, ..
        } = *self
        else {
            return Some(true);
        };
        if pole
            || w.length_squared() == 0.0
            || onward.length_squared() == 0.0
            || back.length_squared() == 0.0
        {
            return None;
        }
        let turn = |from: Point2, to: Point2| {
            let a = from.perp_dot(to).atan2(from.dot(to));
            if a < 0.0 {
                a + std::f64::consts::TAU
            } else {
                a
            }
        };
        // The face is what lies counter-clockwise from the way onward
        // round to the way back.
        let (open, at) = (turn(onward, back), turn(onward, w));
        let open = if open < 1e-9 {
            std::f64::consts::TAU
        } else {
            open
        };
        // (The directions are the curves' own at the vertex, known to
        // rounding: a crossing that leaves the boundary by a millionth
        // of a degree does leave it.)
        const NEAR: f64 = 1e-9;
        if at < NEAR || (at - open).abs() < NEAR || at > std::f64::consts::TAU - NEAR {
            None
        } else {
            Some(at < open)
        }
    }
}

/// A face of one of the solids, as the fuse sees it once edges are cut.
pub(crate) struct Work {
    pub solid: usize,
    pub face: usize,
    /// The walks of its boundary, loop by loop.
    pub halves: Vec<Half>,
    pub loops: Vec<Vec<usize>>,
    /// The parts on it: its boundary's, and those drawn on it.
    pub on: BTreeSet<usize>,
}

pub(crate) struct Fuse<'a> {
    pub solids: [&'a Solid; 2],
    pub data: [SolidData; 2],
    pub verts: Vec<Vert>,
    pub vmap: [Vec<usize>; 2],
    pub parts: Vec<Part>,
    pub chains: [Vec<Vec<Link>>; 2],
    pub scale: f64,
    /// How far a fitted curve may stray.
    pub target: f64,
    /// Vertices inside each face (faces of the first solid, then of the
    /// second), with their (u, v).
    pub inner: Vec<Vec<(usize, Point2)>>,
    /// Parts drawn on each face, with their curves in its (u, v).
    pub extra: Vec<Vec<(usize, Arc<Curve2>)>>,
    pub works: Vec<Work>,
    pub touched: bool,
    /// What each face's surface is told to be, once it is asked.
    known: std::cell::OnceCell<[Vec<Option<crate::known::Known>>; 2]>,
}

fn degenerate(what: impl Into<String>) -> Refusal {
    Refusal::Degenerate(what.into())
}

/// `a` and `b` combined by `op`. Refused, with the reason, where the
/// result is not certain.
pub fn combine(op: Op, a: &Solid, b: &Solid) -> Result<Outcome, Refusal> {
    // Solids far from the origin are combined about it and the result
    // put back: so far out, their coordinates are not kept as closely
    // as crossings are looked for.
    if let Some(by) = crate::place::recentred(&[a, b])
        && let (Ok(a), Ok(b)) = (a.moved(&|p| p - by), b.moved(&|p| p - by))
    {
        let mut made = combine_here(op, &a, &b)?;
        if let Ok(back) = made.solid.moved(&|p| p + by) {
            made.solid = back;
            return Ok(made);
        }
    }
    combine_here(op, a, b)
}

/// How much wider than they are written the tolerances of vertices,
/// edges and faces are taken when two solids could not be combined as
/// they are written.
///
/// Things meet where they come within the sum of their tolerances. Two
/// faces that lie within that of each other are one face there — but
/// their edges are then up to √2 times as far apart (off by as much
/// along two faces), and their corners up to √3 times (along three):
/// with one width for all, the faces are found together and their
/// corners apart, and what is one place is cut into pieces smaller
/// than can be told apart. So edges are taken √2 and vertices √3 times
/// as wide. And all of them a millionth wider still: what is made
/// exactly a tolerance from something is, measured, a rounding to
/// either side of it.
const WIDER: [f64; 3] = [
    1.732_050_807_568_877_2 * (1.0 + 1e-6),
    std::f64::consts::SQRT_2 * (1.0 + 1e-6),
    1.0 + 1e-6,
];

/// How many times wider again, one after the other, while the solids
/// still meet too closely to tell apart: what cannot be told apart at
/// one width is together at a wider one. No wider than this: farther
/// apart than that, things are apart.
const WIDEST: [f64; 4] = [1.0, 4.0, 16.0, 64.0];

/// `solid` with its tolerances `by` times as wide: of its vertices, its
/// edges and its faces.
fn widened(solid: &Solid, by: [f64; 3]) -> Solid {
    let mut solid = solid.clone();
    for v in &mut solid.vertices {
        v.tolerance *= by[0];
    }
    for e in &mut solid.edges {
        e.tolerance *= by[1];
    }
    for f in &mut solid.faces {
        f.tolerance *= by[2];
    }
    solid
}

/// [`combine`], where the solids are: as they are written, and where
/// that is refused because they meet too closely, with their
/// tolerances taken wider (see [`WIDER`]). The result is as narrow as
/// the solids were, but for what it needs to hold its own edges and
/// vertices.
fn combine_here(op: Op, a: &Solid, b: &Solid) -> Result<Outcome, Refusal> {
    let refused = match combine_as_written(op, a, b) {
        Err(why @ (Refusal::Degenerate(_) | Refusal::Open(_))) => why,
        done => return done,
    };
    for times in WIDEST {
        let by = WIDER.map(|w| w * times);
        if let Ok(mut made) = combine_as_written(op, &widened(a, by), &widened(b, by)) {
            made.solid = widened(&made.solid, by.map(|w| 1.0 / w));
            made.solid.settle();
            return Ok(made);
        }
    }
    Err(refused)
}

/// [`combine`], with the tolerances the solids are written with.
fn combine_as_written(op: Op, a: &Solid, b: &Solid) -> Result<Outcome, Refusal> {
    if a.is_empty() || b.is_empty() {
        let solid = match op {
            Op::Union if a.is_empty() => b.clone(),
            Op::Union | Op::Cut => a.clone(),
            Op::Intersect => Solid::default(),
        };
        return Ok(Outcome {
            solid,
            touched: false,
        });
    }
    // (Flat faces laid out around a point — the ends of what is turned
    // — evenly instead: what crosses them is simple then.)
    let (even_a, even_b) = (crate::flat::evened(a), crate::flat::evened(b));
    let (a, b) = (even_a.as_ref().unwrap_or(a), even_b.as_ref().unwrap_or(b));
    let mut fuse = Fuse::new([a, b]);
    let pad = a.tolerance() + b.tolerance();
    if fuse.data[0].bounds.meets(&fuse.data[1].bounds, pad) {
        fuse.vertices_on_edges()?;
        fuse.edges_on_edges()?;
        fuse.vertices_in_faces()?;
        fuse.edges_through_faces()?;
        fuse.walk_boundaries()?;
        fuse.touching_points();
        fuse.faces_through_faces()?;
        fuse.closed_crossings()?;
    } else {
        fuse.walk_boundaries()?;
    }
    let made = crate::cells::assemble(&mut fuse, op)?;
    // Where no crossing was drawn, or nothing or as many faces as the
    // first solid's are left, the result is all or nothing of each
    // solid — and a place well inside each says which it has to be:
    // what the two share holds it if the other solid does, the first
    // without the second if the second does not. Where the pieces were
    // sorted another way than that, the faces lie too nearly on each
    // other to have been told apart (two blocks, one turned a hair).
    let whole = made.solid.is_empty()
        || (op == Op::Cut && made.solid.faces.len() == a.faces.len())
        || fuse.extra.iter().all(Vec::is_empty);
    if op != Op::Union && whole {
        let deep = fuse.scale * 1e-3;
        let well_inside = |s: usize| {
            let (solid, data) = (fuse.solids[s], &fuse.data[s]);
            let all: Vec<usize> = (0..solid.faces.len()).collect();
            (0..solid.faces.len()).find_map(|f| {
                let (p, out) = crate::query::sample(solid, data, f)?;
                let q = p - out * deep;
                (crate::query::inside(solid, data, &all, q) == Some(true)).then_some(q)
            })
        };
        // (Inside or outside only of what it is well clear of: near a
        // face, as inside a thin wall, a place says nothing.)
        let clear = fuse.scale * 1e-5;
        let placed = |solid: &Solid, data: &SolidData, q: Point3| {
            let near = solid
                .faces
                .iter()
                .zip(&data.faces)
                .any(|(face, d)| crate::query::on_face(face, d, q, clear).is_some());
            let all: Vec<usize> = (0..solid.faces.len()).collect();
            (!near)
                .then(|| crate::query::inside(solid, data, &all, q))
                .flatten()
        };
        let held = |s: usize, q: Point3| placed(fuse.solids[s], &fuse.data[s], q);
        let result = (!made.solid.is_empty()).then(|| SolidData::new(&made.solid));
        let kept = |q: Point3| match &result {
            None => Some(false),
            Some(data) => placed(&made.solid, data, q),
        };
        for s in 0..2 {
            let Some(q) = well_inside(s) else {
                continue;
            };
            let (Some(other), Some(got)) = (held(1 - s, q), kept(q)) else {
                continue;
            };
            let wanted = match op {
                Op::Intersect => other,
                _ => s == 0 && !other,
            };
            if got != wanted {
                return Err(degenerate(
                    "solids that lie on each other too nearly to sort their faces",
                ));
            }
        }
    }
    Ok(made)
}

impl<'a> Fuse<'a> {
    fn new(solids: [&'a Solid; 2]) -> Self {
        let data = [SolidData::new(solids[0]), SolidData::new(solids[1])];
        let scale = data[0].bounds.union(data[1].bounds).size().max(1e-12);
        let faces = solids[0].faces.len() + solids[1].faces.len();
        let mut fuse = Self {
            solids,
            data,
            verts: Vec::new(),
            vmap: [Vec::new(), Vec::new()],
            parts: Vec::new(),
            chains: [Vec::new(), Vec::new()],
            scale,
            target: scale * 1e-9,
            inner: vec![Vec::new(); faces],
            extra: vec![Vec::new(); faces],
            works: Vec::new(),
            touched: false,
            known: std::cell::OnceCell::new(),
        };
        for (s, solid) in solids.iter().enumerate() {
            for v in &solid.vertices {
                let id = fuse.vertex(v.point, v.tolerance, s);
                if fuse.verts[id].name.is_none() {
                    fuse.verts[id].name = v.name.clone();
                }
                fuse.vmap[s].push(id);
            }
            for (e, edge) in solid.edges.iter().enumerate() {
                let ends = edge.ends.map(|v| fuse.vmap[s][v]);
                let part = fuse.part(
                    edge.curve.clone(),
                    edge.range,
                    ends,
                    edge.tolerance,
                    Some((s, e)),
                );
                fuse.parts[part].name = edge.name.clone();
                fuse.chains[s].push(vec![Link {
                    part,
                    range: edge.range,
                    reversed: false,
                    foreign: false,
                }]);
            }
        }
        fuse
    }

    /// The number of face `f` of solid `s` among all faces.
    pub(crate) fn fid(&self, s: usize, f: usize) -> usize {
        if s == 0 {
            f
        } else {
            self.solids[0].faces.len() + f
        }
    }

    /// The vertex at `p` with tolerance `r`: one there already whose
    /// ball meets it, or a new one.
    pub(crate) fn vertex(&mut self, p: Point3, r: f64, of: usize) -> usize {
        let found = self
            .verts
            .iter()
            .position(|v| v.point.distance(p) <= v.tol + r);
        let id = found.unwrap_or_else(|| {
            self.verts.push(Vert {
                point: p,
                tol: r,
                name: None,
                of: [false; 2],
            });
            self.verts.len() - 1
        });
        if found.is_some() && !self.verts[id].of[of] {
            self.touched = true;
        }
        if of < 2 {
            self.verts[id].of[of] = true;
        }
        id
    }

    /// A vertex both solids have, at `p`.
    fn meeting(&mut self, p: Point3, r: f64) -> usize {
        let id = self.vertex(p, r, 0);
        self.verts[id].of = [true; 2];
        self.touched = true;
        id
    }

    pub(crate) fn part(
        &mut self,
        curve: Arc<Curve3>,
        range: (f64, f64),
        ends: [usize; 2],
        tol: f64,
        owner: Option<(usize, usize)>,
    ) -> usize {
        let bez = curve_pieces(&*curve, range.0, range.1);
        let bounds = bez.iter().fold(Bounds::EMPTY, |b, p| b.union(p.bounds()));
        self.parts.push(Part {
            curve,
            range,
            ends,
            tol,
            name: None,
            bez,
            bounds,
            alive: true,
            owner,
            shared: false,
        });
        self.parts.len() - 1
    }

    /// Cuts part `p` at `cuts` (parameters, each with the vertex
    /// there); returns the pieces.
    fn cut(&mut self, p: usize, mut cuts: Vec<(f64, usize)>) -> Vec<usize> {
        let (a, b) = self.parts[p].range;
        let ends = self.parts[p].ends;
        let margin = (b - a) * 1e-12;
        cuts.retain(|&(t, v)| t > a + margin && t < b - margin && !ends.contains(&v));
        cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
        cuts.dedup_by(|x, y| x.1 == y.1);
        if cuts.is_empty() {
            return vec![p];
        }
        let Some((s, e)) = self.parts[p].owner else {
            return vec![p];
        };
        self.touched = true;
        let mut stops = vec![(a, ends[0])];
        stops.extend(cuts);
        stops.push((b, ends[1]));
        let mut pieces = Vec::with_capacity(stops.len() - 1);
        for pair in stops.windows(2) {
            let id = self.part(
                self.parts[p].curve.clone(),
                (pair[0].0, pair[1].0),
                [pair[0].1, pair[1].1],
                self.parts[p].tol,
                Some((s, e)),
            );
            pieces.push(id);
        }
        self.parts[pieces[0]].name = self.parts[p].name.take();
        self.parts[p].alive = false;
        let chain = &mut self.chains[s][e];
        if let Some(at) = chain.iter().position(|l| l.part == p) {
            let links = pieces.iter().map(|&id| Link {
                part: id,
                range: self.parts[id].range,
                reversed: false,
                foreign: false,
            });
            chain.splice(at..=at, links);
        }
        pieces
    }

    /// The living parts of solid `s` that are its own.
    fn own_parts(&self, s: usize) -> Vec<usize> {
        self.chains[s]
            .iter()
            .flatten()
            .filter(|l| !l.foreign)
            .map(|l| l.part)
            .collect()
    }

    fn too_close(what: &str) -> Refusal {
        degenerate(format!("{what} run along each other in too many places"))
    }

    // ---- 2: vertices on edges ------------------------------------------

    fn vertices_on_edges(&mut self) -> Result<(), Refusal> {
        for s in 0..2 {
            for p in self.own_parts(s) {
                let mut cuts = Vec::new();
                for (v, vert) in self.verts.iter().enumerate() {
                    let part = &self.parts[p];
                    let tol = vert.tol + part.tol;
                    if !vert.of[1 - s]
                        || part.ends.contains(&v)
                        || !part.bounds.holds(vert.point, tol)
                    {
                        continue;
                    }
                    let found = contacts(
                        &At(vert.point),
                        &[Piece::point(vert.point)],
                        &Along(&part.curve, part.range),
                        &part.bez,
                        tol,
                    )
                    .map_err(|_| Self::too_close("a vertex and an edge"))?;
                    cuts.extend(found.iter().map(|c| (c.b[0], v)));
                }
                for &(_, v) in &cuts {
                    self.verts[v].of = [true; 2];
                }
                self.cut(p, cuts);
            }
        }
        Ok(())
    }

    // ---- 3: edges on edges ---------------------------------------------

    /// Whether parts `p` and `q` are the same curve in space.
    fn same_curve(&self, p: usize, q: usize) -> bool {
        let (a, b) = (&self.parts[p], &self.parts[q]);
        let tol = a.tol + b.tol;
        let on = |x: &Part, y: &Part, f: f64| {
            let at = x.curve.point_at(x.range.0 + (x.range.1 - x.range.0) * f);
            y.bounds.holds(at, tol)
                && contacts(
                    &At(at),
                    &[Piece::point(at)],
                    &Along(&y.curve, y.range),
                    &y.bez,
                    tol,
                )
                .is_ok_and(|c| !c.is_empty())
        };
        [0.5, 0.2, 0.8, 0.35, 0.65].into_iter().all(|f| on(a, b, f)) && on(b, a, 0.5)
    }

    /// Makes part `q` (of the second solid) the same edge as `p` (of
    /// the first).
    fn unite(&mut self, p: usize, q: usize) {
        let Some((s, e)) = self.parts[q].owner else {
            return;
        };
        let (a, b) = (&self.parts[p], &self.parts[q]);
        // Which way `q` runs along `p`: by where its middle's
        // neighbours fall.
        let (_, d) = a.curve.derivative_at(a.middle());
        let at = a.curve.point_at(a.middle());
        let (_, y, _, _) = crate::solve::closest(
            &At(at),
            &Along(&b.curve, b.range),
            [0.0; 2],
            [b.middle(), 0.0],
        );
        let t = y[0];
        let (_, db) = b.curve.derivative_at(t);
        let reversed = d.dot(db) < 0.0;
        let tol = a.tol.max(b.tol);
        let range = b.range;
        self.parts[q].alive = false;
        self.parts[p].tol = tol;
        self.parts[p].shared = true;
        if self.parts[p].name.is_none() {
            self.parts[p].name = self.parts[q].name.take();
        }
        self.touched = true;
        if let Some(link) = self.chains[s][e].iter_mut().find(|l| l.part == q) {
            *link = Link {
                part: p,
                range,
                reversed,
                foreign: true,
            };
        }
    }

    /// Pieces of the two solids with the same ends that are the same
    /// curve become one.
    fn unite_same(&mut self) {
        let mut by_ends: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
        for p in self.own_parts(0) {
            let [a, b] = self.parts[p].ends;
            by_ends.entry((a.min(b), a.max(b))).or_default().push(p);
        }
        for q in self.own_parts(1) {
            let [a, b] = self.parts[q].ends;
            let Some(candidates) = by_ends.get(&(a.min(b), a.max(b))) else {
                continue;
            };
            let same = candidates
                .iter()
                .copied()
                .find(|&p| !self.parts[p].shared && self.same_curve(p, q));
            if let Some(p) = same {
                self.unite(p, q);
            }
        }
    }

    fn edges_on_edges(&mut self) -> Result<(), Refusal> {
        self.unite_same();
        let (first, second) = (self.own_parts(0), self.own_parts(1));
        let mut cuts: BTreeMap<usize, Vec<(f64, usize)>> = BTreeMap::new();
        for &p in &first {
            if self.parts[p].shared {
                continue;
            }
            for &q in &second {
                let (a, b) = (&self.parts[p], &self.parts[q]);
                let tol = a.tol + b.tol;
                if !a.bounds.meets(&b.bounds, tol) {
                    continue;
                }
                let found = contacts(
                    &Along(&a.curve, a.range),
                    &a.bez,
                    &Along(&b.curve, b.range),
                    &b.bez,
                    tol,
                )
                .map_err(|_| Self::too_close("two edges"))?;
                let reach = a.tol.max(b.tol);
                let mut hits: Vec<(f64, f64, Point3, f64)> = Vec::new();
                // Where two edges cross is where they are nearest each
                // other — not just anywhere they are within their
                // tolerances, which found twice (from either side of a
                // joint of one of them) would be two places.
                let crossing = |c: &crate::solve::Contact| {
                    let (x, y, pa, pb) = crate::solve::closest(
                        &Along(&a.curve, a.range),
                        &Along(&b.curve, b.range),
                        [c.a[0], 0.0],
                        [c.b[0], 0.0],
                    );
                    let gap = pa.distance(pb);
                    // (Where they only touch, the nearest pair is known
                    // no better than the root of what they are apart
                    // beside it: there, where they run the same way.)
                    let touch = kissed(
                        (&a.curve, a.range),
                        (&b.curve, b.range),
                        (x[0], y[0]),
                        tol.max(c.distance),
                        self.scale,
                    );
                    if let Some((t, s, pa, pb)) = touch {
                        (t, s, (pa + pb) / 2.0, reach + pa.distance(pb) / 2.0)
                    } else if gap <= tol.max(c.distance) {
                        (x[0], y[0], (pa + pb) / 2.0, reach + gap / 2.0)
                    } else {
                        (c.a[0], c.b[0], c.point(), reach + c.distance / 2.0)
                    }
                };
                for c in found {
                    let (lo, hi) = c.box_a[0];
                    let long = a.curve.point_at(lo).distance(a.curve.point_at(hi)) > 8.0 * tol;
                    if !long {
                        hits.push(crossing(&c));
                        continue;
                    }
                    // Where they run together, both ends of the run.
                    let ends = [lo, hi].map(|t| {
                        let (_, y, pa, pb) = crate::solve::closest(
                            &At(a.curve.point_at(t)),
                            &Along(&b.curve, b.range),
                            [0.0; 2],
                            [c.b[0], 0.0],
                        );
                        (t, y[0], pa, pb)
                    });
                    // Two curves that only touch are within the
                    // tolerance for a stretch too, but part towards
                    // both its ends: that is one point, where they are
                    // closest, not a stretch they share.
                    let gaps = ends.map(|e| e.2.distance(e.3));
                    let apart = gaps.map(|g| g > tol * 0.5 && g > c.distance * 4.0);
                    if apart[0] && apart[1] {
                        hits.push(crossing(&c));
                        continue;
                    }
                    // And where they touch at an end of one of them,
                    // they part towards the other end only, steadily:
                    // half way there they are apart already, as curves
                    // that run together are not.
                    if apart[0] != apart[1] {
                        let (near, far) = if apart[0] { (1, 0) } else { (0, 1) };
                        let middle = a.curve.point_at((lo + hi) / 2.0);
                        let (_, _, pa, pb) = crate::solve::closest(
                            &At(middle),
                            &Along(&b.curve, b.range),
                            [0.0; 2],
                            [(ends[0].1 + ends[1].1) / 2.0, 0.0],
                        );
                        if gaps[near] < gaps[far] * 0.05 && pa.distance(pb) > gaps[far] * 0.15 {
                            let (t, s, pa, pb) = ends[near];
                            hits.push((t, s, (pa + pb) / 2.0, reach + pa.distance(pb) / 2.0));
                            continue;
                        }
                    }
                    for (t, s, pa, pb) in ends {
                        hits.push((t, s, (pa + pb) / 2.0, reach + pa.distance(pb) / 2.0));
                    }
                }
                for (ta, tb, at, r) in hits {
                    let v = self.meeting(at, r);
                    cuts.entry(p).or_default().push((ta, v));
                    cuts.entry(q).or_default().push((tb, v));
                }
            }
        }
        for (p, at) in cuts {
            self.cut(p, at);
        }
        self.unite_same();
        Ok(())
    }

    // ---- 4: vertices in faces ------------------------------------------

    /// The vertices on the boundary of face `f` of solid `s`.
    fn boundary_vertices(&self, s: usize, f: usize) -> BTreeSet<usize> {
        let mut out = BTreeSet::new();
        for step in self.solids[s].faces[f].loops.iter().flatten() {
            match step.walk {
                Walk::Edge { edge, .. } => {
                    for link in &self.chains[s][edge] {
                        out.extend(self.parts[link.part].ends);
                    }
                }
                Walk::Pole { vertex, .. } => {
                    out.insert(self.vmap[s][vertex]);
                }
            }
        }
        out
    }

    /// Where the point `p` is inside face `f` of solid `s`, within
    /// `tol`: its (u, v) there.
    fn inside_face(&self, s: usize, f: usize, p: Point3, tol: f64) -> Option<Point2> {
        let (face, data) = (&self.solids[s].faces[f], &self.data[s].faces[f]);
        if !data.bounds.holds(p, tol) {
            return None;
        }
        let found = contacts(
            &At(p),
            &[Piece::point(p)],
            &Over(&face.surface, data.uv),
            &data.patches,
            tol,
        )
        .ok()?;
        found
            .iter()
            .map(|c| Point2::new(c.b[0], c.b[1]))
            .find(|&uv| data.trim.holds(uv) == Some(true))
    }

    fn vertices_in_faces(&mut self) -> Result<(), Refusal> {
        for s in 0..2 {
            for f in 0..self.solids[s].faces.len() {
                let boundary = self.boundary_vertices(s, f);
                let tol = self.solids[s].faces[f].tolerance;
                let fid = self.fid(s, f);
                for v in 0..self.verts.len() {
                    if !self.verts[v].of[1 - s] || boundary.contains(&v) {
                        continue;
                    }
                    let vert = &self.verts[v];
                    if let Some(uv) = self.inside_face(s, f, vert.point, vert.tol + tol) {
                        self.inner[fid].push((v, uv));
                        self.verts[v].of = [true; 2];
                        self.touched = true;
                    }
                }
            }
        }
        Ok(())
    }

    // ---- 5: edges through faces ----------------------------------------

    fn edges_through_faces(&mut self) -> Result<(), Refusal> {
        // Parts lying in a face, to be drawn on it once they are cut.
        let mut lying: Vec<(usize, usize, usize)> = Vec::new();
        let mut cuts: BTreeMap<usize, Vec<(f64, usize)>> = BTreeMap::new();
        // (The flat faces laid out evenly: an edge clear of such a
        // face's plane, or straight through it, is told without a
        // search.)
        let flats: [Vec<Option<crate::flat::Flat>>; 2] = [0, 1].map(|s| {
            let faces = self.solids[s].faces.iter();
            faces.map(|f| crate::flat::Flat::of(&f.surface)).collect()
        });
        for s in 0..2 {
            let other = 1 - s;
            for p in self.own_parts(s) {
                if self.parts[p].shared {
                    continue;
                }
                for f in 0..self.solids[other].faces.len() {
                    let (face, data) = (&self.solids[other].faces[f], &self.data[other].faces[f]);
                    let part = &self.parts[p];
                    let tol = part.tol + face.tolerance;
                    if !part.bounds.meets(&data.bounds, tol) {
                        continue;
                    }
                    let flat = flats[other].get(f).and_then(Option::as_ref);
                    let quick = flat.and_then(|flat| flat.meets(&part.bez, tol, data.uv));
                    let quick = match quick {
                        Some(crate::flat::Met::Lying) => {
                            lying.push((p, other, f));
                            continue;
                        }
                        Some(crate::flat::Met::Through(found)) => Some(found),
                        None => None,
                    };
                    if quick.as_ref().is_some_and(Vec::is_empty) {
                        continue;
                    }
                    let surface = Over(&face.surface, data.uv);
                    let on = |fr: f64| {
                        let at = part
                            .curve
                            .point_at(part.range.0 + (part.range.1 - part.range.0) * fr);
                        contacts(&At(at), &[Piece::point(at)], &surface, &data.patches, tol)
                            .is_ok_and(|c| !c.is_empty())
                    };
                    let found = match quick {
                        Some(through) => Ok(through
                            .into_iter()
                            .map(|(t, uv, point)| crate::solve::Contact {
                                a: [t, 0.0],
                                b: [uv.x, uv.y],
                                pa: point,
                                pb: point,
                                distance: 0.0,
                                box_a: [(t, t), (0.0, 0.0)],
                                box_b: [(uv.x, uv.x), (uv.y, uv.y)],
                            })
                            .collect()),
                        None => {
                            if on(0.5) && [0.15, 0.85, 0.3, 0.7].into_iter().all(on) {
                                lying.push((p, other, f));
                                continue;
                            }
                            contacts(
                                &Along(&part.curve, part.range),
                                &part.bez,
                                &surface,
                                &data.patches,
                                tol,
                            )
                        }
                    };
                    let Ok(found) = found else {
                        // An edge that lies in the face's surface for a
                        // stretch and leaves it where the surface ends
                        // (a line across the plane of a disk, on past
                        // its rim). Had that stretch been in the face,
                        // the face's own edges would have cut it there:
                        // if none of it is, the two do not meet.
                        // (Its ends are where it was cut: on the
                        // face's edges, if anywhere.)
                        let within = (1..32).any(|k| {
                            let at = part.curve.point_at(
                                part.range.0 + (part.range.1 - part.range.0) * k as f64 / 32.0,
                            );
                            contacts(&At(at), &[Piece::point(at)], &surface, &data.patches, tol)
                                .is_ok_and(|found| {
                                    found.iter().any(|c| {
                                        data.trim.holds(Point2::new(c.b[0], c.b[1])) != Some(false)
                                    })
                                })
                        });
                        if within {
                            return Err(Self::too_close("an edge and a face"));
                        }
                        continue;
                    };
                    let boundary = self.boundary_vertices(other, f);
                    let fid = self.fid(other, f);
                    let reach = part.tol.max(face.tolerance);
                    let found: Vec<_> = found
                        .into_iter()
                        .filter(|c| {
                            let uv = Point2::new(c.b[0], c.b[1]);
                            // An edge that runs into the face along it (a
                            // face a blend leaves as flatly as it lies) is
                            // within the tolerances of it for a stretch
                            // from where it meets the face's edge: a place
                            // found in that stretch is that meeting.
                            let part = &self.parts[p];
                            let (_, along) = part.curve.derivative_at(c.a[0]);
                            let across = normal(&face.surface, uv);
                            let grazing = along.normalize_or_zero().dot(across).abs() < 1e-3;
                            // (The meeting may be the edge's own end,
                            // come to rest in the face.)
                            let met = grazing
                                && boundary.iter().chain(&part.ends).any(|&w| {
                                    let at = self.verts[w].point;
                                    let near = self.verts[w].tol + tol;
                                    at.distance(c.point()) > near
                                        && at.distance(c.point()) <= self.scale * 0.01
                                        && crate::geometry::on_curve(
                                            &part.curve,
                                            part.range,
                                            at,
                                            near,
                                        )
                                        .into_iter()
                                        .any(|t| {
                                            let span = part.range.1 - part.range.0;
                                            // Where the edge goes through
                                            // the face (a hole's wall a
                                            // hair inside a side), it is
                                            // on both sides of it there.
                                            let side = |k: f64| {
                                                let at = (c.a[0] + (c.a[0] - t) * k)
                                                    .clamp(part.range.0, part.range.1);
                                                let q = part.curve.point_at(at);
                                                let (foot, _) =
                                                    crate::geometry::foot(&face.surface, q, uv);
                                                let off = q - face.surface.point_at(foot.x, foot.y);
                                                off.dot(normal(&face.surface, foot))
                                            };
                                            let (before, after) = (side(-1.0), side(1.0));
                                            let through = before * after < 0.0
                                                && before.abs().min(after.abs()) > tol * 0.05;
                                            !through
                                                && [0.25, 0.5, 0.75].into_iter().all(|k| {
                                                    on((t + (c.a[0] - t) * k - part.range.0) / span)
                                                })
                                        })
                                });
                            !met
                        })
                        .collect();
                    for c in found {
                        // (Where the edge goes through the face: where
                        // the two are nearest, not just anywhere they
                        // are within their tolerances of each other.)
                        let (face, data) =
                            (&self.solids[other].faces[f], &self.data[other].faces[f]);
                        let part = &self.parts[p];
                        let (x, y, pa, pb) = crate::solve::closest(
                            &Along(&part.curve, part.range),
                            &Over(&face.surface, data.uv),
                            [c.a[0], 0.0],
                            [c.b[0], c.b[1]],
                        );
                        let gap = pa.distance(pb);
                        let c = if gap <= tol.max(c.distance) {
                            crate::solve::Contact {
                                a: [x[0], 0.0],
                                b: y,
                                pa,
                                pb,
                                distance: gap,
                                ..c
                            }
                        } else {
                            c
                        };
                        let c = grazed(&part.curve, part.range, &face.surface, &c, tol, self.scale)
                            .or_else(|| {
                                through(&part.curve, part.range, &face.surface, &c, tol, self.scale)
                            })
                            .unwrap_or(c);
                        let uv = Point2::new(c.b[0], c.b[1]);
                        let held = self.data[other].faces[f].trim.holds(uv);
                        if held == Some(false) {
                            continue;
                        }
                        // An edge that runs in the face's surface is
                        // within the tolerances of it a little past
                        // where it leaves the face: the place found
                        // there is the vertex it leaves it at, if one
                        // is as near as that.
                        let part = &self.parts[p];
                        let (_, along) = part.curve.derivative_at(c.a[0]);
                        let across = normal(&self.solids[other].faces[f].surface, uv);
                        let grazing = along.normalize_or_zero().dot(across).abs() < 1e-3;
                        let at = c.point();
                        let nearby = grazing
                            .then(|| {
                                (0..self.verts.len())
                                    .filter(|&w| {
                                        at.distance(self.verts[w].point)
                                            <= self.verts[w].tol + 8.0 * (tol + reach)
                                    })
                                    .min_by(|&x, &y| {
                                        let d = |w: usize| at.distance(self.verts[w].point);
                                        d(x).total_cmp(&d(y))
                                    })
                            })
                            .flatten();
                        let v = match nearby {
                            Some(w) => {
                                self.verts[w].of = [true; 2];
                                self.touched = true;
                                w
                            }
                            None => self.meeting(at, reach + c.distance / 2.0),
                        };
                        if !boundary.contains(&v) {
                            if held.is_none() {
                                // On the boundary, where no edge was
                                // found to cross: nothing to cut at.
                                continue;
                            }
                            if !self.inner[fid].iter().any(|x| x.0 == v) {
                                self.inner[fid].push((v, uv));
                            }
                        }
                        cuts.entry(p).or_default().push((c.a[0], v));
                    }
                }
            }
        }
        let mut pieces: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (p, at) in cuts {
            let made = self.cut(p, at);
            pieces.insert(p, made);
        }
        for (p, s, f) in lying {
            for piece in pieces.get(&p).cloned().unwrap_or_else(|| vec![p]) {
                self.draw(piece, s, f)?;
            }
        }
        Ok(())
    }

    /// Draws part `p`, which lies in the surface of face `f` of solid
    /// `s`, on that face — if it is inside it.
    fn draw(&mut self, p: usize, s: usize, f: usize) -> Result<(), Refusal> {
        let face = &self.solids[s].faces[f];
        let part = &self.parts[p];
        let tol = part.tol + face.tolerance;
        let t = part.middle();
        let Some(uv) = self.inside_face(s, f, part.curve.point_at(t), tol) else {
            return Ok(());
        };
        let fitted = project(&face.surface, &part.curve, part.range, (t, uv), self.target)
            .ok_or_else(|| degenerate("an edge lying in a face could not be drawn on it"))?;
        let fid = self.fid(s, f);
        let boundary = self.boundary_vertices(s, f);
        for (end, t) in [(part.ends[0], part.range.0), (part.ends[1], part.range.1)] {
            if !boundary.contains(&end) && !self.inner[fid].iter().any(|x| x.0 == end) {
                self.inner[fid].push((end, fitted.curve.point_at(t)));
            }
        }
        let grown = self.parts[p].tol.max(face.tolerance).max(fitted.off * 1.5);
        self.parts[p].tol = grown;
        self.parts[p].shared = true;
        self.extra[fid].push((p, Arc::new(fitted.curve)));
        self.touched = true;
        Ok(())
    }

    // ---- the faces' boundaries, cut -------------------------------------

    fn walk_boundaries(&mut self) -> Result<(), Refusal> {
        for s in 0..2 {
            for (f, face) in self.solids[s].faces.iter().enumerate() {
                let mut work = Work {
                    solid: s,
                    face: f,
                    halves: Vec::new(),
                    loops: Vec::new(),
                    on: BTreeSet::new(),
                };
                for steps in &face.loops {
                    let mut walk = Vec::new();
                    for step in steps {
                        match step.walk {
                            Walk::Pole { vertex, range } => {
                                let v = self.vmap[s][vertex];
                                walk.push(work.halves.len());
                                work.halves.push(Half {
                                    part: None,
                                    forward: true,
                                    tail: v,
                                    head: v,
                                    pcurve: step.pcurve.clone(),
                                    range,
                                });
                            }
                            Walk::Edge { edge, reversed } => {
                                let chain = &self.chains[s][edge];
                                let order: Vec<&Link> = if reversed {
                                    chain.iter().rev().collect()
                                } else {
                                    chain.iter().collect()
                                };
                                for link in order {
                                    let part = &self.parts[link.part];
                                    let forward = reversed == link.reversed;
                                    let pcurve = if link.foreign {
                                        let own = &self.solids[s].edges[edge];
                                        let fitted = reparametrize(
                                            &face.surface,
                                            &step.pcurve,
                                            &own.curve,
                                            link.range,
                                            &part.curve,
                                            part.range,
                                            link.reversed,
                                            self.target,
                                        )
                                        .ok_or_else(|| {
                                            degenerate("an edge of both bodies could not be drawn on a face")
                                        })?;
                                        Arc::new(fitted.curve)
                                    } else {
                                        step.pcurve.clone()
                                    };
                                    let [a, b] = part.ends;
                                    let (tail, head) = if forward { (a, b) } else { (b, a) };
                                    walk.push(work.halves.len());
                                    work.on.insert(link.part);
                                    work.halves.push(Half {
                                        part: Some(link.part),
                                        forward,
                                        tail,
                                        head,
                                        pcurve,
                                        range: part.range,
                                    });
                                }
                            }
                        }
                    }
                    work.loops.push(walk);
                }
                let fid = self.fid(s, f);
                work.on.extend(self.extra[fid].iter().map(|x| x.0));
                self.works.push(work);
            }
        }
        Ok(())
    }

    /// Where vertex `v` is on face `fid`.
    pub(crate) fn sites(&self, fid: usize, v: usize) -> Vec<Site> {
        let work = &self.works[fid];
        let mut out = Vec::new();
        for walk in &work.loops {
            for (k, &h) in walk.iter().enumerate() {
                let (arriving, leaving) =
                    (&work.halves[h], &work.halves[walk[(k + 1) % walk.len()]]);
                if arriving.head != v {
                    continue;
                }
                // The way the boundary goes at the vertex itself: its
                // curve's own direction there (a chord from the vertex
                // leans by half the curve's bend along it, which is
                // more than a crossing that all but runs along the
                // boundary leaves it by); where that is nothing, the
                // chords.
                let direction = |half: &Half, from_head: bool| {
                    let (_, d) = half
                        .pcurve
                        .derivative_at(half.at(f64::from(u8::from(from_head))));
                    let d = if half.forward != from_head { d } else { -d };
                    let (a, b) = half.range;
                    let reach =
                        half.uv(0.0).distance(half.uv(0.5)) + half.uv(0.5).distance(half.uv(1.0));
                    if d.length() * (b - a).abs() > reach * 1e-6 {
                        return d;
                    }
                    [1e-3, 1e-2, 1e-1]
                        .into_iter()
                        .map(|f| half.leaving(from_head, f))
                        .find(|d| d.length_squared() > 0.0)
                        .unwrap_or(Point2::ZERO)
                };
                out.push(Site::Corner {
                    uv: leaving.uv(0.0),
                    onward: direction(leaving, false),
                    back: direction(arriving, true),
                    pole: arriving.part.is_none() || leaving.part.is_none(),
                });
            }
        }
        out.extend(
            self.inner[fid]
                .iter()
                .filter(|x| x.0 == v)
                .map(|x| Site::Inner { uv: x.1 }),
        );
        out
    }

    /// The vertices with a place on face `fid`.
    fn vertices_of(&self, fid: usize) -> BTreeSet<usize> {
        let mut out: BTreeSet<usize> = self.inner[fid].iter().map(|x| x.0).collect();
        out.extend(self.works[fid].halves.iter().map(|h| h.head));
        out
    }

    // ---- 6: faces through faces ----------------------------------------

    /// Whether `p` is on a part that lies on both faces `fa` and `fb`
    /// and ends at `v`.
    fn drawn(&self, fa: usize, fb: usize, v: Option<usize>, p: Point3, lean: f64) -> bool {
        self.works[fa]
            .on
            .intersection(&self.works[fb].on)
            .any(|&q| {
                let part = &self.parts[q];
                // A part within its tolerance of both faces is further
                // than that from where they cross, the shallower they
                // cross (`lean`, the sine of the angle).
                let tol = 2.0 * part.tol / lean.clamp(0.01, 1.0) + self.target * 8.0;
                v.is_none_or(|v| part.ends.contains(&v))
                    && part.bounds.holds(p, tol)
                    && contacts(
                        &At(p),
                        &[Piece::point(p)],
                        &Along(&part.curve, part.range),
                        &part.bez,
                        tol,
                    )
                    .is_ok_and(|c| !c.is_empty())
            })
    }

    /// Whether a part that lies on both faces `fa` and `fb` leaves
    /// vertex `v` the way `way` points: the crossing that way is drawn
    /// already.
    fn leaves(&self, fa: usize, fb: usize, v: usize, way: Point3) -> bool {
        self.works[fa]
            .on
            .intersection(&self.works[fb].on)
            .any(|&q| {
                let part = &self.parts[q];
                let (a, b) = part.range;
                let step = (b - a) * 1e-3;
                let from_start = part.ends[0] == v && {
                    let d = part.curve.point_at(a + step) - part.curve.point_at(a);
                    d.normalize_or_zero().dot(way) > 0.996
                };
                let from_end = part.ends[1] == v && {
                    let d = part.curve.point_at(b - step) - part.curve.point_at(b);
                    d.normalize_or_zero().dot(way) > 0.996
                };
                from_start || from_end
            })
    }

    /// Follows the crossing of faces `fa` (of the first solid) and `fb`
    /// from vertex `v`, every way it leaves it into both faces, and
    /// draws what it finds on both.
    fn follow_from(&mut self, fa: usize, fb: usize, v: usize) -> Result<(), Refusal> {
        let (wa, wb) = (&self.works[fa], &self.works[fb]);
        let (face_a, face_b) = (
            &self.solids[0].faces[wa.face],
            &self.solids[1].faces[wb.face],
        );
        let (surface_a, surface_b) = (face_a.surface.clone(), face_b.surface.clone());
        let tracer = Tracer {
            a: &surface_a,
            b: &surface_b,
            target: self.target,
            scale: self.scale,
            first: self.scale * 0.02,
        };
        let at = self.verts[v].point;
        let reach = self.verts[v].tol + self.target * 4.0;
        let goals: Vec<Goal> = self
            .vertices_of(fa)
            .intersection(&self.vertices_of(fb))
            .map(|&w| (w, self.verts[w].point, self.verts[w].tol))
            .collect();
        // How far to look ahead before deciding: well short of the next
        // vertex.
        let nearest = goals
            .iter()
            .filter(|g| g.0 != v)
            .map(|g| g.1.distance(at))
            .fold(f64::INFINITY, f64::min);
        let probe = (self.scale * 1e-3).min(nearest * 0.3).max(reach * 16.0);
        for site_a in self.sites(fa, v) {
            for site_b in self.sites(fb, v) {
                let (ua, ub) = (site_a.uv(), site_b.uv());
                let across = normal(&surface_a, ua).cross(normal(&surface_b, ub));
                // Where the faces touch and branches of their crossing
                // meet, the ways out are the branches'; elsewhere, the
                // two ways along the one crossing.
                let touching = across.length() <= 1e-7;
                let (start, ways) = if touching {
                    let rays = branches(&surface_a, ua, &surface_b, ub, self.scale);
                    (
                        Node {
                            p: at,
                            a: ua,
                            b: ub,
                            tangent: Point3::ZERO,
                        },
                        rays,
                    )
                } else {
                    // Where a surface ends short of the vertex (an edge
                    // off it by its tolerance), the crossing starts a
                    // little way in from it.
                    let way = across.normalize();
                    let start = tracer.correct(ua, ub, at, way).or_else(|| {
                        [1.0, -1.0, 2.0, -2.0]
                            .into_iter()
                            .find_map(|k| tracer.correct(ua, ub, at + way * (k * reach), way))
                    });
                    let Some(start) = start else {
                        continue;
                    };
                    if start.p.distance(at) > 2.0 * reach / across.length().clamp(0.1, 1.0)
                        || start.tangent == Point3::ZERO
                    {
                        continue;
                    }
                    (start, vec![start.tangent, -start.tangent])
                };
                for tangent in ways {
                    let (da, db) = (
                        pull(&surface_a, start.a, tangent),
                        pull(&surface_b, start.b, tangent),
                    );
                    let (into_a, into_b) = (site_a.enters(da), site_b.enters(db));
                    if into_a == Some(false) || into_b == Some(false) {
                        continue;
                    }
                    if self.leaves(fa, fb, v, tangent) {
                        continue;
                    }
                    let (data_a, data_b) = (
                        &self.data[0].faces[self.works[fa].face],
                        &self.data[1].faces[self.works[fb].face],
                    );
                    let held = |inside: Option<bool>, into: Option<bool>| match inside {
                        Some(x) => x,
                        // On the boundary by a hair: the corner decides.
                        None => into == Some(true),
                    };
                    // A look ahead along the crossing: is it inside
                    // both faces there?
                    let look = |probe: f64| {
                        tracer
                            .correct(
                                start.a + da * probe,
                                start.b + db * probe,
                                start.p + tangent * probe,
                                tangent,
                            )
                            .filter(|n| {
                                held(data_a.trim.holds(n.a), into_a)
                                    && held(data_b.trim.holds(n.b), into_b)
                            })
                    };
                    // Where that finds nothing and the faces cross
                    // squarely, the crossing may be a loop smaller than
                    // the look ahead: how tightly it bends is seen from
                    // a very short step, and it is looked ahead on at
                    // its own size.
                    let tiny = (reach * 4.0).max(self.scale * 1e-7);
                    // Not past a vertex the crossing runs straight to:
                    // a stretch shorter than the look ahead is looked
                    // at in its middle.
                    let probe = goals
                        .iter()
                        .filter(|g| g.0 != v)
                        .map(|g| (g.1 - at, g.1.distance(at)))
                        .filter(|(d, far)| *far < probe * 2.0 && d.dot(tangent) > 0.99 * far)
                        .fold(probe, |least, (_, far)| least.min(far * 0.5));
                    let ahead = look(probe).map(|n| (n, probe)).or_else(|| {
                        if touching || across.length() < 2e-3 {
                            return None;
                        }
                        let near = tracer
                            .correct(
                                start.a + da * tiny,
                                start.b + db * tiny,
                                start.p + tangent * tiny,
                                tangent,
                            )
                            .filter(|n| n.tangent != Point3::ZERO)?;
                        let turned = near.tangent.cross(tangent).length().max(1e-12);
                        let radius = near.p.distance(start.p) / turned;
                        let short = (radius * 0.05).max(tiny);
                        (short < probe).then(|| look(short))?.map(|n| (n, short))
                    });
                    let Some((ahead, looked)) = ahead else {
                        continue;
                    };
                    // (Where branches meet, the faces touch at the
                    // vertex itself, and that says nothing of how they
                    // cross along a branch.)
                    let lean = if touching { 0.1 } else { across.length() };
                    if self.drawn(fa, fb, Some(v), ahead.p, lean) {
                        continue;
                    }
                    // How tightly it bends, from the look ahead: it is
                    // followed in steps of its own size, as closely as
                    // its bend asks for — but no closer than faces
                    // crossing at a shallow angle let a point of their
                    // crossing be found.
                    let radius = if touching || ahead.tangent == Point3::ZERO {
                        f64::INFINITY
                    } else {
                        let turned = ahead.tangent.cross(tangent).length().max(1e-12);
                        let radius = ahead.p.distance(start.p) / turned;
                        // A bend much tighter than the look ahead is
                        // long would not have been looked ahead on:
                        // that is noise, where faces all but touch.
                        if radius >= looked * 0.5 {
                            radius
                        } else {
                            f64::INFINITY
                        }
                    };
                    let tracer = Tracer {
                        target: self
                            .target
                            .min(radius * 1e-9)
                            .max(self.scale * 1e-14 / across.length().max(1e-7)),
                        first: (self.scale * 0.02).min(radius * 0.1),
                        ..tracer
                    };
                    let inside = |n: &Node| {
                        data_a.trim.holds(n.a) != Some(false)
                            && data_b.trim.holds(n.b) != Some(false)
                    };
                    let first = Node { tangent, ..start };
                    let (nodes, stop) = tracer.follow(first, v, &goals, &inside);
                    let end = match stop {
                        Stop::Vertex(w) => w,
                        // Where the faces all but touch, what looked
                        // like a crossing is their touching: nothing
                        // to draw.
                        Stop::Lost(_) if across.length() < 1e-3 => continue,
                        // From a point a surface collapses to (a pole)
                        // there is no telling which way in (u, v) the
                        // crossing leaves; it is followed from its
                        // other end, to here.
                        Stop::Lost(_) if site_a.at_pole() || site_b.at_pole() => {
                            continue;
                        }
                        Stop::Lost(why) => {
                            return Err(degenerate(format!(
                                "where {} and {} cross: {why}",
                                face_a.name.as_deref().unwrap_or("a face"),
                                face_b.name.as_deref().unwrap_or("a face"),
                            )));
                        }
                    };
                    let (curve, pa, pb, off) = tracer
                        .curves(&nodes)
                        .ok_or_else(|| degenerate("a crossing could not be fitted"))?;
                    let range = curve.domain();
                    let tol = face_a.tolerance.max(face_b.tolerance).max(off * 1.5);
                    // The vertices hold the ends of what was drawn — but
                    // what would lie within the two of them from end to
                    // end is no edge: where faces all but touch along
                    // an edge they share, their crossing is found again
                    // beside it, in pieces of no length.
                    let held = [(v, range.0), (end, range.1)].map(|(w, t)| {
                        let gap = curve.point_at(t).distance(self.verts[w].point);
                        (w, self.verts[w].tol.max(gap * (1.0 + 1e-9)))
                    });
                    // A vertex is no wider than what is near: a
                    // crossing that ends farther from one than that
                    // does not end at it.
                    if held
                        .iter()
                        .any(|&(w, tol)| tol > (self.verts[w].tol * 1e3).max(self.scale * 1e-6))
                    {
                        return Err(degenerate(format!(
                            "where {} and {} cross: it ends away from the vertex it comes to",
                            face_a.name.as_deref().unwrap_or("a face"),
                            face_b.name.as_deref().unwrap_or("a face"),
                        )));
                    }
                    if v != end {
                        let apart = self.verts[v].point.distance(self.verts[end].point);
                        let within = held[0].1 + held[1].1;
                        let long = nodes.iter().any(|n| {
                            n.p.distance(self.verts[v].point) > within
                                && n.p.distance(self.verts[end].point) > within
                        });
                        if apart <= within && !long {
                            continue;
                        }
                    }
                    for (w, tol) in held {
                        self.verts[w].tol = tol;
                    }
                    let part = self.part(Arc::new(curve), range, [v, end], tol, None);
                    self.parts[part].shared = true;
                    self.extra[fa].push((part, Arc::new(pa)));
                    self.extra[fb].push((part, Arc::new(pb)));
                    self.works[fa].on.insert(part);
                    self.works[fb].on.insert(part);
                    self.touched = true;
                }
            }
        }
        Ok(())
    }

    /// Whether one of the faces `fa` and `fb` (of the two solids) is
    /// known to run on smoothly into the other, along an edge of its
    /// own that lies in the other.
    fn smooth(&self, fa: usize, fb: usize) -> bool {
        [(fa, fb), (fb, fa)].into_iter().any(|(own, other)| {
            let work = &self.works[own];
            let known = &self.solids[work.solid].faces[work.face].smooth;
            !known.is_empty()
                && self.works[other].on.iter().any(|&p| {
                    self.parts[p]
                        .owner
                        .is_some_and(|(s, e)| s == work.solid && known.contains(&e))
                })
        })
    }

    fn faces_through_faces(&mut self) -> Result<(), Refusal> {
        let (na, nb) = (self.solids[0].faces.len(), self.solids[1].faces.len());
        let of: Vec<BTreeSet<usize>> = (0..na + nb).map(|fid| self.vertices_of(fid)).collect();
        for fa in 0..na {
            shapecraft_core::cancel::checkpoint();
            for fb in na..na + nb {
                let tol =
                    self.solids[0].faces[fa].tolerance + self.solids[1].faces[fb - na].tolerance;
                if !self.data[0].faces[fa]
                    .bounds
                    .meets(&self.data[1].faces[fb - na].bounds, tol)
                {
                    continue;
                }
                if self.smooth(fa, fb) {
                    continue;
                }
                let both: Vec<usize> = of[fa].intersection(&of[fb]).copied().collect();
                for v in both {
                    self.follow_from(fa, fb, v)?;
                }
            }
        }
        Ok(())
    }

    /// What each face's surface is, where that is told: by solid.
    fn known(&self) -> [Vec<Option<crate::known::Known>>; 2] {
        self.known
            .get_or_init(|| {
                [0, 1].map(|k| {
                    self.data[k]
                        .faces
                        .iter()
                        .map(|f| crate::known::known(&f.patches))
                        .collect()
                })
            })
            .clone()
    }

    // ---- points where faces touch ---------------------------------------

    /// Finds the points inside two faces where they touch and branches
    /// of their crossing meet (two cylinders of one size, crossed), and
    /// makes each a vertex, for the branches to be followed to and
    /// from.
    fn touching_points(&mut self) {
        let na = self.solids[0].faces.len();
        let known = self.known();
        for fa in 0..na {
            shapecraft_core::cancel::checkpoint();
            for fb in na..na + self.solids[1].faces.len() {
                // (A plane and anything, cylinders side by side: where
                // they touch, it is along a line.)
                if let (Some(a), Some(b)) = (&known[0][fa], &known[1][fb - na])
                    && crate::known::never_branch(a, b)
                {
                    continue;
                }
                let (face_a, face_b) = (&self.solids[0].faces[fa], &self.solids[1].faces[fb - na]);
                let (data_a, data_b) = (&self.data[0].faces[fa], &self.data[1].faces[fb - na]);
                let tol = face_a.tolerance + face_b.tolerance;
                if !data_a.bounds.meets(&data_b.bounds, tol) || self.smooth(fa, fb) {
                    continue;
                }
                let (over_a, over_b) = (
                    Over(&face_a.surface, data_a.uv),
                    Over(&face_b.surface, data_b.uv),
                );
                let mut near = Vec::new();
                let hulls = (
                    crate::loops::Hull::of(&data_a.patches),
                    crate::loops::Hull::of(&data_b.patches),
                );
                let hulls = (&hulls.0, &hulls.1);
                for pa in &data_a.patches {
                    for pb in &data_b.patches {
                        crate::loops::touches(&over_a, pa, &over_b, pb, hulls, tol, &mut near);
                    }
                }
                // Each place is come at from many sides; more than a
                // few places means the faces touch along a curve, which
                // is no crossing.
                let mut places: Vec<(Point2, Point2, Point3)> = Vec::new();
                let mut misses = 0;
                for (ua, ub) in near {
                    let guess = face_a.surface.point_at(ua.x, ua.y);
                    if places
                        .iter()
                        .any(|f| f.2.distance(guess) <= self.scale * 0.02)
                    {
                        continue;
                    }
                    let Some((ua, ub)) =
                        touching(&face_a.surface, &face_b.surface, ua, ub, tol, self.scale)
                    else {
                        misses += 1;
                        if misses > 48 {
                            break;
                        }
                        continue;
                    };
                    let p = face_a.surface.point_at(ua.x, ua.y);
                    if !places.iter().any(|f| f.2.distance(p) <= tol * 4.0) {
                        places.push((ua, ub, p));
                    }
                    if places.len() > 16 {
                        break;
                    }
                }
                if places.len() > 16 {
                    continue;
                }
                let mut found: Vec<(Point2, Point2, Point3)> = Vec::new();
                for (ua, ub, p) in places {
                    if data_a.trim.holds(ua) != Some(true) || data_b.trim.holds(ub) != Some(true) {
                        continue;
                    }
                    if !branches(&face_a.surface, ua, &face_b.surface, ub, self.scale).is_empty() {
                        found.push((ua, ub, p));
                    }
                }
                for (ua, ub, p) in found {
                    // Beside an edge the two faces share, they touch
                    // all along it (a fillet and the face it runs into,
                    // met by themselves): no point where branches meet.
                    if self.drawn(fa, fb, None, p, 0.0) {
                        continue;
                    }
                    let v = self.meeting(p, tol);
                    if !self.vertices_of(fa).contains(&v) {
                        self.inner[fa].push((v, ua));
                    }
                    if !self.vertices_of(fb).contains(&v) {
                        self.inner[fb].push((v, ub));
                    }
                }
            }
        }
    }

    // ---- closed crossings no edge reaches -------------------------------

    fn closed_crossings(&mut self) -> Result<(), Refusal> {
        let na = self.solids[0].faces.len();
        // Faces told for what they are — two planes, a plane along a
        // cylinder, cylinders side by side — cross in straight lines,
        // which leave both through their edges: never in a closed
        // curve.
        let known = self.known();
        for fa in 0..na {
            shapecraft_core::cancel::checkpoint();
            for fb in na..na + self.solids[1].faces.len() {
                let (face_a, face_b) = (&self.solids[0].faces[fa], &self.solids[1].faces[fb - na]);
                let (data_a, data_b) = (&self.data[0].faces[fa], &self.data[1].faces[fb - na]);
                let tol = face_a.tolerance + face_b.tolerance;
                if !data_a.bounds.meets(&data_b.bounds, tol) || self.smooth(fa, fb) {
                    continue;
                }
                let mut seeds = Vec::new();
                // Faces told for what they are — planes, cylinders,
                // cones, spheres — say what they cross in by
                // themselves: nothing closed (straight lines, curves
                // that run on to an edge), or a circle, of which one
                // place is enough.
                let mut told = false;
                if let (Some(a), Some(b)) = (&known[0][fa], &known[1][fb - na]) {
                    use crate::known::Closed;
                    let at = (data_a.patches[0].at(0, 0), data_b.patches[0].at(0, 0));
                    match crate::known::closed(a, b, at, tol) {
                        Closed::Never => continue,
                        Closed::Through(places) => {
                            let mut all = Vec::new();
                            for p in places {
                                let on =
                                    |face: &crate::model::Face, data: &crate::query::FaceData| {
                                        let over = Over(&face.surface, data.uv);
                                        let found = contacts(
                                            &At(p),
                                            &[Piece::point(p)],
                                            &over,
                                            &data.patches,
                                            tol,
                                        )
                                        .ok()?;
                                        // (Of the two sides of a seam, the
                                        // one farther in.)
                                        let inside = |c: &crate::solve::Contact| {
                                            (0..2)
                                                .map(|k| {
                                                    let (lo, hi) = data.uv[k];
                                                    (c.b[k] - lo).min(hi - c.b[k]) / (hi - lo)
                                                })
                                                .fold(f64::INFINITY, f64::min)
                                        };
                                        found
                                            .iter()
                                            .map(|c| (inside(c), Point2::new(c.b[0], c.b[1])))
                                            .max_by(|p, q| p.0.total_cmp(&q.0))
                                            .map(|found| found.1)
                                    };
                                if let (Some(ua), Some(ub)) =
                                    (on(face_a, data_a), on(face_b, data_b))
                                {
                                    all.push((p, ua, ub));
                                }
                            }
                            // (All three, or it is looked for: a place
                            // not found on a face may be past its rim.
                            // And all three near each other in both
                            // faces' (u, v): a curve that goes around
                            // a face that closes on itself is drawn
                            // from where it crosses the seam, which is
                            // where looking for it starts.)
                            let together =
                                |of: &dyn Fn(&(Point3, Point2, Point2)) -> Point2,
                                 uv: [(f64, f64); 2]| {
                                    (0..2).all(|k| {
                                        let at = all.iter().map(|x| of(x).to_array()[k]);
                                        let (lo, hi) = at
                                            .fold((f64::INFINITY, f64::NEG_INFINITY), |r, x| {
                                                (r.0.min(x), r.1.max(x))
                                            });
                                        hi - lo <= 0.2 * (uv[k].1 - uv[k].0)
                                    })
                                };
                            if all.len() == 3
                                && together(&|x| x.1, data_a.uv)
                                && together(&|x| x.2, data_b.uv)
                            {
                                seeds = all;
                                told = true;
                            }
                        }
                        Closed::Unknown => {}
                    }
                }
                // (How much looking past the usual depth this pair gets.)
                let mut left = if told { 0 } else { 1_500 };
                let hulls = (
                    crate::loops::Hull::of(&data_a.patches),
                    crate::loops::Hull::of(&data_b.patches),
                );
                let hulls = (&hulls.0, &hulls.1);
                for pa in &data_a.patches {
                    if told {
                        break;
                    }
                    for pb in &data_b.patches {
                        // (A face with loops: its patches reach no
                        // farther than what the loops lie in.)
                        crate::loops::seeds(
                            (
                                &Over(&face_a.surface, data_a.uv),
                                !data_a.trim.pieces.is_empty(),
                            ),
                            pa,
                            (
                                &Over(&face_b.surface, data_b.uv),
                                !data_b.trim.pieces.is_empty(),
                            ),
                            pb,
                            hulls,
                            tol,
                            &mut left,
                            &mut seeds,
                        );
                    }
                }
                for (p, ua, ub) in seeds {
                    let (data_a, data_b) = (&self.data[0].faces[fa], &self.data[1].faces[fb - na]);
                    if data_a.trim.holds(ua) != Some(true) || data_b.trim.holds(ub) != Some(true) {
                        continue;
                    }
                    let lean = {
                        let (sa, sb) = (
                            &self.solids[0].faces[fa].surface,
                            &self.solids[1].faces[fb - na].surface,
                        );
                        normal(sa, ua).cross(normal(sb, ub)).length()
                    };
                    if self.drawn(fa, fb, None, p, lean) {
                        continue;
                    }
                    let v = self.meeting(p, tol);
                    if !self.vertices_of(fa).contains(&v) {
                        self.inner[fa].push((v, ua));
                    }
                    if !self.vertices_of(fb).contains(&v) {
                        self.inner[fb].push((v, ub));
                    }
                    self.follow_from(fa, fb, v)?;
                }
            }
        }
        Ok(())
    }
}

/// Where two curves only touch, near the parameters `at` of a place
/// they are within `tol` of each other: where they run the same way —
/// the turn of one against the other changes sides there, a plain root,
/// while how far apart they are has a flat bottom, known no closer than
/// the root of rounding. Its parameters on both, and the two points.
/// `None` where they cross, or do not turn against each other.
fn kissed(
    a: (&Curve3, (f64, f64)),
    b: (&Curve3, (f64, f64)),
    at: (f64, f64),
    tol: f64,
    scale: f64,
) -> Option<(f64, f64, Point3, Point3)> {
    let ((ca, ra), (cb, rb)) = (a, b);
    let speed = ca.derivative_at(at.0).1.length();
    if speed <= 0.0 {
        return None;
    }
    // The place on the second nearest a point: from the search's own
    // answer by Newton's steps (all but on it, as good as exact).
    let beside = |q: Point3, guess: f64| {
        let (_, y, _, _) = crate::solve::closest(&At(q), &Along(cb, rb), [0.0; 2], [guess, 0.0]);
        let mut s = y[0];
        for _ in 0..4 {
            let (p, d) = cb.derivative_at(s);
            let dd = d.length_squared();
            if dd <= 0.0 {
                break;
            }
            let next = (s + (q - p).dot(d) / dd).clamp(rb.0, rb.1);
            if next == s {
                break;
            }
            s = next;
        }
        s
    };
    let ways = |t: f64| {
        let (q, da) = ca.derivative_at(t);
        let s = beside(q, at.1);
        let db = cb.derivative_at(s).1;
        (da.normalize_or_zero().cross(db.normalize_or_zero()), s)
    };
    // Touching: all but the same way where they are found together.
    if ways(at.0).0.length() >= 1e-3 {
        return None;
    }
    let reach = (scale * 1e-5).max(tol * 100.0) / speed;
    let (mut lo, mut hi) = ((at.0 - reach).max(ra.0), (at.0 + reach).min(ra.1));
    // The turn, measured against the side it is on at the low end.
    let across = ways(lo).0.try_normalize()?;
    let turn = |t: f64| ways(t).0.dot(across);
    let (mut tl, mut th) = (turn(lo), turn(hi));
    if tl == 0.0 || th == 0.0 || (tl > 0.0) == (th > 0.0) {
        return None;
    }
    for _ in 0..80 {
        let guess = lo - tl * (hi - lo) / (th - tl);
        let middle = (lo + hi) / 2.0;
        let t = if (guess - middle).abs() < (hi - lo) / 4.0 {
            guess
        } else {
            middle
        };
        let now = turn(t);
        if (now > 0.0) == (tl > 0.0) {
            (lo, tl) = (t, now);
        } else {
            (hi, th) = (t, now);
        }
        if now == 0.0 || (hi - lo) * speed <= scale * 1e-14 {
            break;
        }
    }
    let t = if tl.abs() <= th.abs() { lo } else { hi };
    let pa = ca.point_at(t);
    let s = beside(pa, at.1);
    let pb = cb.point_at(s);
    (pa.distance(pb) <= tol).then_some((t, s, pa, pb))
}

/// The (u, v) of the point of `surface` nearest `q`, near `guess`, as
/// closely as rounding lets it be told: the search's own answer, and
/// from there by Newton's steps on the surface's two directions being
/// square to the way to `q` — which, for a point all but on the
/// surface, as good as square the error each time. (The normal there
/// is then as exact, where the search's alone leaves it a hundred
/// millionth off.)
fn sharp_foot(surface: &NurbsSurface, q: Point3, guess: Point2) -> Point2 {
    let (mut uv, _) = crate::geometry::foot(surface, q, guess);
    let ((u0, u1), (v0, v1)) = surface.domain();
    for _ in 0..4 {
        let (p, su, sv) = surface.derivatives_at(uv.x, uv.y);
        let off = q - p;
        let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let det = a * c - b * b;
        if det <= 1e-24 * (a + c) * (a + c) || !det.is_finite() {
            break;
        }
        let (r, t) = (su.dot(off), sv.dot(off));
        let step = Point2::new((r * c - t * b) / det, (t * a - r * b) / det);
        let next = Point2::new((uv.x + step.x).clamp(u0, u1), (uv.y + step.y).clamp(v0, v1));
        if next == uv {
            break;
        }
        uv = next;
    }
    uv
}

/// Where `curve` goes through `surface`, near the contact `c`: the
/// place where it is on neither side of it. A curve that crosses a
/// surface at a shallow angle is within any tolerance of it for a long
/// stretch, and the nearest pair of points is found anywhere along it;
/// the side of the surface it is on changes at one place, however
/// shallow the crossing. `None` where the curve stays on one side.
fn through(
    curve: &Curve3,
    range: (f64, f64),
    surface: &NurbsSurface,
    c: &crate::solve::Contact,
    tol: f64,
    scale: f64,
) -> Option<crate::solve::Contact> {
    let t = c.a[0];
    let (_, along) = curve.derivative_at(t);
    let speed = along.length();
    if speed <= 0.0 {
        return None;
    }
    let guess = Point2::new(c.b[0], c.b[1]);
    let lean = along.dot(normal(surface, guess)).abs() / speed;
    // (Squarely enough through it, the nearest pair is the place.)
    if lean >= 1e-3 {
        return None;
    }
    // How far off the surface the curve is, to the side its normal
    // points.
    let side = |t: f64| {
        let q = curve.point_at(t);
        let (foot, _) = crate::geometry::foot(surface, q, guess);
        (q - surface.point_at(foot.x, foot.y)).dot(normal(surface, foot))
    };
    // As far either way as the curve stays within the tolerance of the
    // surface, and a little farther.
    let reach = (tol * 4.0 / lean.max(1e-12)).min(scale) / speed;
    let (mut a, mut b) = ((t - reach).max(range.0), (t + reach).min(range.1));
    let (mut sa, mut sb) = (side(a), side(b));
    if sa == 0.0 || sb == 0.0 || (sa > 0.0) == (sb > 0.0) {
        return None;
    }
    for _ in 0..80 {
        let guess = a - sa * (b - a) / (sb - sa);
        let middle = (a + b) / 2.0;
        let at = if (guess - middle).abs() < (b - a) / 4.0 {
            guess
        } else {
            middle
        };
        let s = side(at);
        if (s > 0.0) == (sa > 0.0) {
            (a, sa) = (at, s);
        } else {
            (b, sb) = (at, s);
        }
        if s == 0.0 || (b - a) * speed <= scale * 1e-14 {
            break;
        }
    }
    let t = if sa.abs() <= sb.abs() { a } else { b };
    let pa = curve.point_at(t);
    let (foot, gap) = crate::geometry::foot(surface, pa, guess);
    (gap <= tol.max(c.distance)).then(|| crate::solve::Contact {
        a: [t, 0.0],
        b: foot.to_array(),
        pa,
        pb: surface.point_at(foot.x, foot.y),
        distance: gap,
        ..c.clone()
    })
}

/// Where `curve` only touches `surface`, near the contact `c`: the
/// place where it runs square to the surface's normal. Where a curve
/// grazes a surface, the two are within any tolerance of each other for
/// a stretch about the root of it long, and the nearest pair of points
/// is known no better than that; the turn of the curve against the
/// normal has a plain root in the middle of the stretch. `None` where
/// the curve does not graze the surface there, or goes through it.
fn grazed(
    curve: &Curve3,
    range: (f64, f64),
    surface: &NurbsSurface,
    c: &crate::solve::Contact,
    tol: f64,
    scale: f64,
) -> Option<crate::solve::Contact> {
    let t = c.a[0];
    let (_, along) = curve.derivative_at(t);
    let speed = along.length();
    if speed <= 0.0 {
        return None;
    }
    let guess = Point2::new(c.b[0], c.b[1]);
    if along.dot(normal(surface, guess)).abs() >= 1e-3 * speed {
        return None;
    }
    // The curve's turn against the surface's normal at its foot there.
    let lean = |t: f64| {
        let (q, d) = curve.derivative_at(t);
        let foot = sharp_foot(surface, q, guess);
        d.normalize_or_zero().dot(normal(surface, foot))
    };
    let reach = (scale * 1e-5).max(tol * 100.0) / speed;
    let (mut a, mut b) = ((t - reach).max(range.0), (t + reach).min(range.1));
    let (mut la, mut lb) = (lean(a), lean(b));
    if la == 0.0 || lb == 0.0 || (la > 0.0) == (lb > 0.0) {
        return None;
    }
    for _ in 0..60 {
        // By the secant where that halves the stretch at least; else
        // by halves.
        let guess = a - la * (b - a) / (lb - la);
        let middle = (a + b) / 2.0;
        let quarter = (b - a) / 4.0;
        let at = if (guess - middle).abs() < quarter {
            guess
        } else {
            middle
        };
        let l = lean(at);
        if (l > 0.0) == (la > 0.0) {
            (a, la) = (at, l);
        } else {
            (b, lb) = (at, l);
        }
        if l == 0.0 || (b - a) * speed <= scale * 1e-14 {
            break;
        }
    }
    let t = if la.abs() <= lb.abs() { a } else { b };
    let pa = curve.point_at(t);
    let (foot, gap) = crate::geometry::foot(surface, pa, guess);
    // (A curve that goes through the surface leans one way on either
    // side of it too, but is not nearest where that turns.)
    (gap <= tol.max(c.distance)).then(|| crate::solve::Contact {
        a: [t, 0.0],
        b: foot.to_array(),
        pa,
        pb: surface.point_at(foot.x, foot.y),
        distance: gap,
        ..c.clone()
    })
}

/// The (u, v) on two surfaces, near `ua` and `ub`, of a point where they
/// touch: are together, with one normal. By damped least squares on the
/// gap between them and the cross of their normals.
fn touching(
    sa: &NurbsSurface,
    sb: &NurbsSurface,
    ua: Point2,
    ub: Point2,
    tol: f64,
    scale: f64,
) -> Option<(Point2, Point2)> {
    let clamp = |s: &NurbsSurface, q: Point2| {
        let ((u0, u1), (v0, v1)) = s.domain();
        Point2::new(q.x.clamp(u0, u1), q.y.clamp(v0, v1))
    };
    // A surface's point and normal at a place: asked for once for
    // both, and of one surface only where the other's place stays.
    let at = |s: &NurbsSurface, q: [f64; 2]| -> (Point3, Point3) {
        let q = Point2::new(q[0], q[1]);
        let (p, du, dv) = s.derivatives_at(q.x, q.y);
        let across = du.cross(dv);
        match across.try_normalize() {
            Some(n) if across.length() > 1e-9 * du.length().max(dv.length()).powi(2) => (p, n),
            _ => (p, normal(s, q)),
        }
    };
    let both = |z: [f64; 4]| (at(sa, [z[0], z[1]]), at(sb, [z[2], z[3]]));
    let residual_of = |(pa, na): (Point3, Point3), (pb, nb): (Point3, Point3)| -> [f64; 6] {
        let gap = pa - pb;
        let lean = na.cross(nb) * scale;
        [gap.x, gap.y, gap.z, lean.x, lean.y, lean.z]
    };
    let norm = |r: &[f64; 6]| r.iter().map(|x| x * x).sum::<f64>();
    let fit = |z: [f64; 4]| {
        let (a, b) = (
            clamp(sa, Point2::new(z[0], z[1])),
            clamp(sb, Point2::new(z[2], z[3])),
        );
        [a.x, a.y, b.x, b.y]
    };
    let widths = {
        let (da, db) = (sa.domain(), sb.domain());
        [
            da.0.1 - da.0.0,
            da.1.1 - da.1.0,
            db.0.1 - db.0.0,
            db.1.1 - db.1.0,
        ]
    };
    let mut z = fit([ua.x, ua.y, ub.x, ub.y]);
    let mut here = both(z);
    let mut r = residual_of(here.0, here.1);
    for _ in 0..60 {
        let n = norm(&r);
        if n.sqrt() <= 1e-13 * scale {
            break;
        }
        // The change of the residual with each parameter, by
        // differences.
        let mut j = [[0.0; 6]; 4];
        for k in 0..4 {
            let h = widths[k] * 1e-7;
            let (mut up, mut down) = (z, z);
            up[k] += h;
            down[k] -= h;
            let (up, down) = (fit(up), fit(down));
            let (ru, rd) = if k < 2 {
                (
                    residual_of(at(sa, [up[0], up[1]]), here.1),
                    residual_of(at(sa, [down[0], down[1]]), here.1),
                )
            } else {
                (
                    residual_of(here.0, at(sb, [up[2], up[3]])),
                    residual_of(here.0, at(sb, [down[2], down[3]])),
                )
            };
            let span = up[k] - down[k];
            if span == 0.0 {
                return None;
            }
            for i in 0..6 {
                j[k][i] = (ru[i] - rd[i]) / span;
            }
        }
        let mut h = [[0.0; 4]; 4];
        let mut g = [0.0; 4];
        for a in 0..4 {
            for b in 0..4 {
                h[a][b] = (0..6).map(|i| j[a][i] * j[b][i]).sum();
            }
            g[a] = -(0..6).map(|i| j[a][i] * r[i]).sum::<f64>();
        }
        let trace: f64 = (0..4).map(|a| h[a][a]).sum();
        let mut damping = 1e-12;
        let mut moved = false;
        while damping < 1e6 {
            let mut hd = h;
            for (a, row) in hd.iter_mut().enumerate() {
                row[a] += damping * (row[a] + trace * 1e-9);
            }
            if let Some(d) = crate::solve::solve(hd, g, 4) {
                let next = fit([z[0] + d[0], z[1] + d[1], z[2] + d[2], z[3] + d[3]]);
                let there = both(next);
                let rn = residual_of(there.0, there.1);
                if norm(&rn) < n {
                    (z, r, here) = (next, rn, there);
                    moved = true;
                    break;
                }
            }
            damping *= 100.0;
        }
        if !moved {
            break;
        }
    }
    let (a, b) = (Point2::new(z[0], z[1]), Point2::new(z[2], z[3]));
    let gap = sa.point_at(a.x, a.y).distance(sb.point_at(b.x, b.y));
    let lean = normal(sa, a).cross(normal(sb, b)).length();
    (gap <= tol && lean <= 1e-6).then_some((a, b))
}

/// The ways the crossing of two surfaces leaves a point where they
/// touch (at `ua` on the first, `ub` on the second): none where they
/// only touch, four where two branches cross. Seen from their common
/// tangent plane, the gap between them is, to second order, the
/// difference of how they curve; the branches run where that is zero.
fn branches(
    sa: &NurbsSurface,
    ua: Point2,
    sb: &NurbsSurface,
    ub: Point2,
    scale: f64,
) -> Vec<Point3> {
    let n = normal(sa, ua);
    let (e1, e2) = n.any_orthonormal_pair();
    let step = scale * 1e-4;
    // How a surface bends away from the plane along `d`.
    let bend = |s: &NurbsSurface, uv: Point2, d: Point3| {
        let w = pull(s, uv, d) * step;
        let at = |q: Point2| s.point_at(q.x, q.y);
        // Across a seam, where the surface closes on itself, it goes
        // on from its other side.
        let ((u0, u1), (v0, v1)) = s.domain();
        let closed = |along_u: bool| {
            [0.25, 0.5, 0.75].into_iter().all(|f| {
                let (lo, hi) = if along_u {
                    let v = v0 + (v1 - v0) * f;
                    (at(Point2::new(u0, v)), at(Point2::new(u1, v)))
                } else {
                    let u = u0 + (u1 - u0) * f;
                    (at(Point2::new(u, v0)), at(Point2::new(u, v1)))
                };
                lo.distance(hi) <= scale * 1e-12
            })
        };
        let wrapped = |q: Point2| {
            let mut q = q;
            if (q.x < u0 || q.x > u1) && closed(true) {
                q.x = u0 + (q.x - u0).rem_euclid(u1 - u0);
            }
            if (q.y < v0 || q.y > v1) && closed(false) {
                q.y = v0 + (q.y - v0).rem_euclid(v1 - v0);
            }
            at(q)
        };
        n.dot(wrapped(uv + w) + wrapped(uv - w) - at(uv) * 2.0) / (step * step)
    };
    let gap = |d: Point3| bend(sa, ua, d) - bend(sb, ub, d);
    let (m11, m22) = (gap(e1), gap(e2));
    let m12 = gap((e1 + e2).normalize()) - (m11 + m22) / 2.0;
    let size = m11.abs().max(m22.abs()).max(m12.abs());
    let det = m11 * m22 - m12 * m12;
    // Clearly a saddle between them: faces that touch all along a curve
    // (a round running smoothly into a flat) curve alike along it, and
    // have no branches there.
    // (Nor where they lie flatter on each other than their bends can be
    // told apart at all: a face that leaves another with no curvature,
    // as a curvature-continuous round does, touches it there and has
    // no branches to follow.)
    if size * scale < 0.05 || det >= -1e-3 * size * size {
        return Vec::new();
    }
    let root = (-det).sqrt();
    // Where m11·x² + 2·m12·x·y + m22·y² = 0.
    let lines: [(f64, f64); 2] = if m11.abs() >= m22.abs() {
        [((-m12 + root) / m11, 1.0), ((-m12 - root) / m11, 1.0)]
    } else {
        [(1.0, (-m12 + root) / m22), (1.0, (-m12 - root) / m22)]
    };
    lines
        .into_iter()
        .flat_map(|(x, y)| {
            let d = (e1 * x + e2 * y).normalize();
            [d, -d]
        })
        .collect()
}
