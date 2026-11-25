//! The body a mesh makes: its faces as flat (or, for a quad that is
//! not flat, ruled) faces between straight edges, or — subdivided —
//! one bicubic NURBS face for each quad of the smooth surface it
//! controls, the faces sharing the curves of their edges. A closed
//! mesh makes a solid, an open one a sheet.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Context;
use workpiece_boolean::Solid;
use workpiece_boolean::model::{Edge as SolidEdge, Face, Use, Vertex, Walk};
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{Curve2, Curve3, NurbsSurface};

use crate::ops;
use crate::poly::{Poly, edge};
use crate::subd;

/// A piece of a mesh as a body, and whether it is closed: a solid.
pub struct Piece {
    /// Its vertices, edges and faces.
    pub solid: Solid,
    /// Whether it encloses a volume.
    pub closed: bool,
}

/// The mesh ready to be made faces of: what faces nothing is gone,
/// and its faces are turned to face one way.
fn sound(poly: &Poly) -> Poly {
    let mut made = poly.clone();
    // (The lines make curves of their own.)
    made.lines.clear();
    made.corners.clear();
    made.tidy();
    let floor = made.size() * 1e-9;
    let thin: std::collections::BTreeSet<usize> = (0..made.faces.len())
        .filter(|&f| {
            let ring = &made.faces[f];
            let distinct: std::collections::BTreeSet<usize> = ring.iter().copied().collect();
            distinct.len() != ring.len() || made.area_normal(f).length() <= floor * floor
        })
        .collect();
    let (made, _) = made.without_faces(&thin);
    ops::outwards(&made)
}

/// The faces of `poly` that hang together across edges, piece by
/// piece.
fn pieces_of(poly: &Poly) -> Vec<Vec<usize>> {
    let sides = poly.edges();
    let mut seen = vec![false; poly.faces.len()];
    let mut all = Vec::new();
    for start in 0..poly.faces.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut piece = vec![start];
        let mut todo = vec![start];
        while let Some(f) = todo.pop() {
            let ring = &poly.faces[f];
            for k in 0..ring.len() {
                for side in &sides[&edge(ring[k], ring[(k + 1) % ring.len()])] {
                    if !seen[side.face] {
                        seen[side.face] = true;
                        piece.push(side.face);
                        todo.push(side.face);
                    }
                }
            }
        }
        piece.sort_unstable();
        all.push(piece);
    }
    all
}

/// Builds a solid face by face: its vertices and edges are made when
/// the first face asks for them.
struct Maker<'a> {
    id: &'a str,
    tolerance: f64,
    solid: Solid,
    vertices: BTreeMap<usize, usize>,
    edges: BTreeMap<(usize, usize), usize>,
    /// How often each edge is walked, each way.
    walked: Vec<[usize; 2]>,
}

impl Maker<'_> {
    fn vertex(&mut self, v: usize, at: Point3) -> usize {
        let next = self.solid.vertices.len();
        *self.vertices.entry(v).or_insert_with(|| {
            self.solid.vertices.push(Vertex {
                point: at,
                tolerance: self.tolerance,
                name: None,
            });
            next
        })
    }

    /// The step of a loop along the edge from point `from` to point
    /// `to`, which the surface has from `uv_from` to `uv_to`; `curve`
    /// gives the edge's curve from its lower point to its higher.
    fn step(
        &mut self,
        (from, to): (usize, usize),
        (uv_from, uv_to): (Point2, Point2),
        ends: (Point3, Point3),
        curve: &dyn Fn() -> anyhow::Result<Curve3>,
    ) -> anyhow::Result<Use> {
        let reversed = from > to;
        let key = edge(from, to);
        let (lo, hi) = if reversed { (ends.1, ends.0) } else { ends };
        let (a, b) = (self.vertex(key.0, lo), self.vertex(key.1, hi));
        let found = match self.edges.get(&key) {
            Some(&found) => found,
            None => {
                self.solid.edges.push(SolidEdge {
                    curve: Arc::new(curve()?),
                    range: (0.0, 1.0),
                    ends: [a, b],
                    tolerance: self.tolerance,
                    name: None,
                    also: Vec::new(),
                });
                self.walked.push([0, 0]);
                self.edges.insert(key, self.solid.edges.len() - 1);
                self.solid.edges.len() - 1
            }
        };
        self.walked[found][usize::from(reversed)] += 1;
        let (first, last) = if reversed {
            (uv_to, uv_from)
        } else {
            (uv_from, uv_to)
        };
        Ok(Use {
            walk: Walk::Edge {
                edge: found,
                reversed,
            },
            pcurve: Arc::new(Curve2::line(first, last)?),
        })
    }

    fn face(&mut self, surface: NurbsSurface, walk: Vec<Use>, name: String, tolerance: f64) {
        self.solid.faces.push(Face {
            surface: Arc::new(surface),
            outward: true,
            loops: vec![walk],
            tolerance: self.tolerance + tolerance,
            name: Some(format!("{}/{name}", self.id)),
            also: Vec::new(),
            smooth: Vec::new(),
        });
    }

    fn done(self) -> anyhow::Result<Piece> {
        anyhow::ensure!(
            self.walked.iter().all(|w| w[0] <= 1 && w[1] <= 1),
            "more than two faces of the mesh meet in an edge, or two beside each other face opposite ways: a body cannot be made of it"
        );
        let closed = self.walked.iter().all(|w| *w == [1, 1]);
        Ok(Piece {
            solid: self.solid,
            closed,
        })
    }
}

/// The flat-faced body of the faces `piece` of `poly`.
fn faceted(poly: &Poly, piece: &[usize], id: &str, tolerance: f64) -> anyhow::Result<Piece> {
    let mut maker = Maker {
        id,
        tolerance,
        solid: Solid::default(),
        vertices: BTreeMap::new(),
        edges: BTreeMap::new(),
        walked: Vec::new(),
    };
    for &f in piece {
        let ring = &poly.faces[f];
        let at: Vec<Point3> = ring.iter().map(|&v| poly.points[v]).collect();
        let n = ring.len();
        // The surface, and where each corner is on it.
        let (surface, uv, off): (NurbsSurface, Vec<Point2>, f64) = if n == 3 {
            let surface = NurbsSurface::bilinear(at[0], at[1], at[2], at[1] + at[2] - at[0])?;
            let uv = vec![Point2::ZERO, Point2::X, Point2::Y];
            (surface, uv, 0.0)
        } else if n == 4 {
            let surface = NurbsSurface::bilinear(at[0], at[1], at[3], at[2])?;
            let uv = vec![Point2::ZERO, Point2::X, Point2::ONE, Point2::Y];
            (surface, uv, 0.0)
        } else {
            let (normal, middle) = ops::plane_of(&at);
            // (Facing as the face does.)
            let normal = if normal.dot(poly.area_normal(f)) < 0.0 {
                -normal
            } else {
                normal
            };
            let x = normal.any_orthonormal_vector();
            let y = normal.cross(x);
            let flat: Vec<Point2> = at
                .iter()
                .map(|p| Point2::new((*p - middle).dot(x), (*p - middle).dot(y)))
                .collect();
            let off = at
                .iter()
                .map(|p| (*p - middle).dot(normal).abs())
                .fold(0.0, f64::max);
            let lo = flat.iter().fold(Point2::splat(f64::MAX), |a, b| a.min(*b));
            let hi = flat.iter().fold(Point2::splat(f64::MIN), |a, b| a.max(*b));
            let span = (hi - lo).max(Point2::splat(1e-12));
            let place = |u: f64, v: f64| middle + x * (lo.x + span.x * u) + y * (lo.y + span.y * v);
            let surface = NurbsSurface::bilinear(
                place(0.0, 0.0),
                place(1.0, 0.0),
                place(0.0, 1.0),
                place(1.0, 1.0),
            )?;
            let uv = flat.iter().map(|p| (*p - lo) / span).collect();
            (surface, uv, off)
        };
        let mut walk = Vec::with_capacity(n);
        for k in 0..n {
            let next = (k + 1) % n;
            let (a, b) = (ring[k], ring[next]);
            let (lo, hi) = (poly.points[a.min(b)], poly.points[a.max(b)]);
            let line = move || Ok(Curve3::line(lo, hi)?);
            walk.push(maker.step((a, b), (uv[k], uv[next]), (at[k], at[next]), &line)?);
        }
        maker.face(surface, walk, format!("f{f}"), off);
    }
    maker.done()
}

/// The smooth body of the faces `piece` of `poly`, all quads: a
/// bicubic face for each, where the surface is the plain bicubic one
/// (see [`subd::regular`]) — and four for a face with a corner where
/// it is not, of the mesh one step finer, so that the surface is only
/// come close to in the quarter of the face at that corner.
fn smooth(poly: &Poly, piece: &[usize], id: &str, tolerance: f64) -> anyhow::Result<Piece> {
    let mut maker = Maker {
        id,
        tolerance,
        solid: Solid::default(),
        vertices: BTreeMap::new(),
        edges: BTreeMap::new(),
        walked: Vec::new(),
    };
    let coarse = subd::patches(poly);
    let (finer, middle_of) = subd::refined_with(poly);
    let fine = subd::patches(&finer);
    let plain = subd::regular(poly);
    let whole = |f: usize| poly.faces[f].iter().all(|&v| plain[v]);
    // The edges that are in two halves: those of a face in four.
    let mut halved = std::collections::BTreeSet::new();
    for &f in piece {
        if !whole(f) {
            let ring = &poly.faces[f];
            for k in 0..4 {
                halved.insert(edge(ring[k], ring[(k + 1) % 4]));
            }
        }
    }
    let knots = || vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let surface_of = |grid: [[Point3; 4]; 4]| {
        let control: Vec<Point3> = grid.iter().flatten().copied().collect();
        NurbsSurface::new(3, 3, knots(), knots(), 4, 4, control, None)
    };
    let curve_of = |patches: &subd::Patches, a: usize, b: usize| {
        let (lo, hi) = (a.min(b), a.max(b));
        let control = vec![
            patches.corners[lo],
            patches.along[&(lo, hi)],
            patches.along[&(hi, lo)],
            patches.corners[hi],
        ];
        Ok(Curve3::new(3, knots(), control, None)?)
    };
    let uv = [Point2::ZERO, Point2::X, Point2::ONE, Point2::Y];
    for &f in piece {
        let ring = &poly.faces[f];
        if whole(f) {
            let surface = surface_of(coarse.grid(f, ring))?;
            let mut walk = Vec::with_capacity(8);
            for k in 0..4 {
                let next = (k + 1) % 4;
                let (a, b) = (ring[k], ring[next]);
                if halved.contains(&edge(a, b)) {
                    let m = middle_of[&edge(a, b)];
                    let half = (uv[k] + uv[next]) / 2.0;
                    let at = |v: usize| fine.corners[v];
                    walk.push(maker.step((a, m), (uv[k], half), (at(a), at(m)), &|| {
                        curve_of(&fine, a, m)
                    })?);
                    walk.push(maker.step((m, b), (half, uv[next]), (at(m), at(b)), &|| {
                        curve_of(&fine, m, b)
                    })?);
                } else {
                    let ends = (fine.corners[a], fine.corners[b]);
                    let curve = || curve_of(&coarse, a, b);
                    walk.push(maker.step((a, b), (uv[k], uv[next]), ends, &curve)?);
                }
            }
            maker.face(surface, walk, format!("f{f}"), 0.0);
            continue;
        }
        for quarter in 0..4 {
            let g = 4 * f + quarter;
            let ring = &finer.faces[g];
            let surface = surface_of(fine.grid(g, ring))?;
            let mut walk = Vec::with_capacity(4);
            for k in 0..4 {
                let next = (k + 1) % 4;
                let (a, b) = (ring[k], ring[next]);
                let ends = (fine.corners[a], fine.corners[b]);
                let curve = || curve_of(&fine, a, b);
                walk.push(maker.step((a, b), (uv[k], uv[next]), ends, &curve)?);
            }
            maker.face(surface, walk, format!("f{f}.{quarter}"), 0.0);
        }
    }
    maker.done()
}

/// The bodies of `poly`, one for each piece of it that hangs
/// together, their faces called `{id}/f0`, `{id}/f1`, …: flat-faced,
/// or (`smooth`) the subdivision surface it controls, after `finer`
/// more steps of subdivision than it takes to make quads of it —
/// more, smaller faces, closer to the true surface around the points
/// where not four quads meet. Its faces are called after the quads:
/// `{id}/f3`, or — a quad made four faces of — `{id}/f3.0` …
pub fn bodies(
    poly: &Poly,
    smooth_surface: bool,
    finer: usize,
    id: &str,
) -> anyhow::Result<Vec<Piece>> {
    let mut mesh = sound(poly);
    anyhow::ensure!(!mesh.faces.is_empty(), "the mesh has no faces");
    let tolerance = mesh.size() * 1e-10;
    if !smooth_surface {
        // A face of five corners or more that is not flat is made
        // triangles of, about its middle.
        let bent: Vec<crate::poly::Item> = (0..mesh.faces.len())
            .filter(|&f| {
                let ring = &mesh.faces[f];
                if ring.len() <= 4 {
                    return false;
                }
                let at: Vec<Point3> = ring.iter().map(|&v| mesh.points[v]).collect();
                let (normal, middle) = ops::plane_of(&at);
                at.iter()
                    .any(|p| (*p - middle).dot(normal).abs() > mesh.size() * 1e-7)
            })
            .map(crate::poly::Item::Face)
            .collect();
        if !bent.is_empty() {
            let (cut, _) = ops::triangulate(&mesh, &bent, true).map_err(anyhow::Error::msg)?;
            mesh = cut;
        }
        return pieces_of(&mesh)
            .iter()
            .map(|piece| faceted(&mesh, piece, id, tolerance))
            .collect();
    }
    let all_quads = |mesh: &Poly| mesh.faces.iter().all(|ring| ring.len() == 4);
    if !all_quads(&mesh) {
        mesh = subd::refined(&mesh);
    }
    for _ in 0..finer.min(3) {
        mesh = subd::refined(&mesh);
    }
    anyhow::ensure!(
        mesh.faces.len() <= 20_000,
        "the mesh is too fine to make a surface of ({} faces)",
        mesh.faces.len()
    );
    pieces_of(&mesh)
        .iter()
        .map(|piece| smooth(&mesh, piece, id, tolerance))
        .collect::<anyhow::Result<Vec<_>>>()
        .context("the surface of the mesh")
}
