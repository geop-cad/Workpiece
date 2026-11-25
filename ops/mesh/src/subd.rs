//! Subdivision: the smooth surface a mesh controls. One step of
//! Catmull and Clark's rule makes a finer mesh of quads; the surface
//! the steps go towards is given as one bicubic patch for each quad
//! (after Loop and Schaefer): the very surface where four quads meet
//! at every corner, and close to it around the points where they do
//! not.
//!
//! Creased edges and the rim of an open mesh stay sharp: along them
//! the surface ends in the curve their points control, and a point
//! where more than two of them meet is a corner.

use std::collections::{BTreeMap, BTreeSet};

use shapecraft_core::math::Point3;

use crate::poly::{Edge, Poly, edge};

/// What a point is to the surface.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    /// The surface is smooth around it.
    Smooth,
    /// A crease runs through it, from the one of these points to the
    /// other.
    Crease(usize, usize),
    /// It is a corner: the surface goes through it.
    Corner,
}

/// The edges of a mesh that stay sharp, and what each point is by
/// them.
struct Sharp {
    edges: BTreeSet<Edge>,
    kinds: Vec<Kind>,
}

fn sharp(poly: &Poly) -> Sharp {
    let sides = poly.edges();
    let edges: BTreeSet<Edge> = sides
        .iter()
        .filter(|(e, s)| s.len() != 2 || poly.creases.contains(e))
        .map(|(&e, _)| e)
        .collect();
    let mut ends: Vec<Vec<usize>> = vec![Vec::new(); poly.points.len()];
    for &(a, b) in &edges {
        ends[a].push(b);
        ends[b].push(a);
    }
    let kinds = ends
        .iter()
        .map(|far| match far[..] {
            [] | [_] => Kind::Smooth,
            [a, b] => Kind::Crease(a, b),
            _ => Kind::Corner,
        })
        .collect();
    Sharp { edges, kinds }
}

/// One step of subdivision: every face cut in quads about a new point
/// in its middle and one on each of its edges, all moved towards the
/// smooth surface.
pub fn refined(poly: &Poly) -> Poly {
    refined_with(poly).0
}

/// [`refined`], and the new point on each edge: the old points keep
/// their places among the new, and the faces made of face `f` (a
/// quad) are the four from `4 f` on, each at the corner of its
/// number.
pub fn refined_with(poly: &Poly) -> (Poly, BTreeMap<Edge, usize>) {
    let sides = poly.edges();
    let Sharp { edges, kinds } = sharp(poly);
    let n = poly.points.len();
    let mut made = Poly::default();
    // The old points first (those of no face stay as they are), then
    // a point for each face, then one for each edge.
    let face_point = |f: usize| n + f;
    let centers: Vec<Point3> = (0..poly.faces.len()).map(|f| poly.center(f)).collect();
    let mut edge_point: BTreeMap<Edge, usize> = BTreeMap::new();
    let mut edge_points = Vec::new();
    for (k, (&(a, b), beside)) in sides.iter().enumerate() {
        let middle = (poly.points[a] + poly.points[b]) / 2.0;
        let at = if edges.contains(&(a, b)) {
            middle
        } else {
            let faces: Point3 = beside.iter().map(|s| centers[s.face]).sum();
            (middle + faces / 2.0) / 2.0
        };
        edge_point.insert((a, b), n + poly.faces.len() + k);
        edge_points.push(at);
    }
    let beside = poly.beside();
    let at = poly.faces_at();
    for v in 0..n {
        let p = poly.points[v];
        let moved = match kinds[v] {
            _ if at[v].is_empty() => p,
            Kind::Corner => p,
            Kind::Crease(a, b) => (poly.points[a] + 6.0 * p + poly.points[b]) / 8.0,
            Kind::Smooth => {
                let count = beside[v].len() as f64;
                let faces = at[v].iter().map(|&f| centers[f]).sum::<Point3>() / at[v].len() as f64;
                let edges = beside[v]
                    .iter()
                    .map(|&w| (p + poly.points[w]) / 2.0)
                    .sum::<Point3>()
                    / count;
                (faces + 2.0 * edges + (count - 3.0) * p) / count
            }
        };
        made.points.push(moved);
    }
    made.points.extend(centers);
    made.points.extend(edge_points);
    for (f, ring) in poly.faces.iter().enumerate() {
        let m = ring.len();
        for k in 0..m {
            let (prev, v, next) = (ring[(k + m - 1) % m], ring[k], ring[(k + 1) % m]);
            made.faces.push(vec![
                v,
                edge_point[&edge(v, next)],
                face_point(f),
                edge_point[&edge(prev, v)],
            ]);
        }
    }
    for &(a, b) in &poly.creases {
        if let Some(&middle) = edge_point.get(&(a, b)) {
            made.creases.insert(edge(a, middle));
            made.creases.insert(edge(middle, b));
        }
    }
    (made, edge_point)
}

/// Whether the surface around each point of `poly` is the plain
/// bicubic one: four quads meet there and no crease, or it is on a
/// crease (or the rim) that runs straight through, two quads on each
/// side of it. Around the other points the patches only come close to
/// the surface.
pub fn regular(poly: &Poly) -> Vec<bool> {
    let Sharp { kinds, .. } = sharp(poly);
    let beside = poly.beside();
    let at = poly.faces_at();
    (0..poly.points.len())
        .map(|v| match kinds[v] {
            Kind::Smooth => beside[v].len() == 4 && at[v].len() == 4,
            Kind::Corner => false,
            Kind::Crease(a, b) => {
                let rim = beside[v].len() == 3 && at[v].len() == 2;
                let shared = at[v].iter().any(|&f| {
                    let ring = &poly.faces[f];
                    ring.contains(&a) && ring.contains(&b)
                });
                rim || (beside[v].len() == 4 && at[v].len() == 4 && !shared)
            }
        })
        .collect()
}

/// The surface of a mesh of quads, as the control points of a bicubic
/// Bézier patch for each face.
#[derive(Debug, Clone, Default)]
pub struct Patches {
    /// Where the surface is at each point of the mesh.
    pub corners: Vec<Point3>,
    /// The control point of each edge next to its one end: by that
    /// end and the other.
    pub along: BTreeMap<(usize, usize), Point3>,
    /// The four control points inside each face: the one near each of
    /// its corners, in their order.
    pub inside: Vec<[Point3; 4]>,
}

impl Patches {
    /// The control points of the patch of the face around `ring`, four
    /// by four: the first index along from its first corner to its
    /// second, the second from its first to its last.
    pub fn grid(&self, f: usize, ring: &[usize]) -> [[Point3; 4]; 4] {
        let [v0, v1, v2, v3] = [ring[0], ring[1], ring[2], ring[3]];
        let c = |v: usize| self.corners[v];
        let e = |a: usize, b: usize| self.along[&(a, b)];
        let i = self.inside[f];
        [
            [c(v0), e(v0, v3), e(v3, v0), c(v3)],
            [e(v0, v1), i[0], i[3], e(v3, v2)],
            [e(v1, v0), i[1], i[2], e(v2, v3)],
            [c(v1), e(v1, v2), e(v2, v1), c(v2)],
        ]
    }
}

/// The patches of `poly`, whose faces are all quads.
pub fn patches(poly: &Poly) -> Patches {
    let Sharp { edges, kinds } = sharp(poly);
    let beside = poly.beside();
    let n = poly.points.len();
    let at = |v: usize| poly.points[v];
    // Around each point: the points across the faces at it from it.
    let mut across = vec![Point3::ZERO; n];
    // For each edge from a point: twice the points beside it in the
    // two faces along the edge, and the points across those.
    let mut wings: BTreeMap<(usize, usize), Point3> = BTreeMap::new();
    for ring in &poly.faces {
        for k in 0..4 {
            let (v, next, far, prev) = (
                ring[k],
                ring[(k + 1) % 4],
                ring[(k + 2) % 4],
                ring[(k + 3) % 4],
            );
            across[v] += at(far);
            *wings.entry((v, next)).or_default() += 2.0 * at(prev) + at(far);
            *wings.entry((v, prev)).or_default() += 2.0 * at(next) + at(far);
        }
    }
    let mut made = Patches::default();
    for v in 0..n {
        let corner = match kinds[v] {
            Kind::Corner => at(v),
            Kind::Crease(a, b) => (at(a) + 4.0 * at(v) + at(b)) / 6.0,
            Kind::Smooth => {
                let count = beside[v].len() as f64;
                if count == 0.0 {
                    at(v)
                } else {
                    let around: Point3 = beside[v].iter().map(|&w| at(w)).sum();
                    (count * count * at(v) + 4.0 * around + across[v]) / (count * (count + 5.0))
                }
            }
        };
        made.corners.push(corner);
    }
    // How many edges meet at a point, as the rules count them: at a
    // crease or a corner the surface is as beside a regular rim.
    let count = |v: usize| match kinds[v] {
        Kind::Smooth => beside[v].len() as f64,
        _ => 4.0,
    };
    for v in 0..n {
        for &w in &beside[v] {
            let sharp_here = edges.contains(&edge(v, w)) && kinds[v] != Kind::Smooth;
            let point = if sharp_here {
                (2.0 * at(v) + at(w)) / 3.0
            } else {
                let c = count(v);
                let wing = wings.get(&(v, w)).copied().unwrap_or_default();
                (2.0 * c * at(v) + 4.0 * at(w) + wing) / (2.0 * c + 10.0)
            };
            made.along.insert((v, w), point);
        }
    }
    // Around a smooth point where not four edges meet, the control
    // points beside it are brought into one plane with it: the
    // patches around it then lie against one plane there.
    for v in 0..n {
        if kinds[v] != Kind::Smooth || beside[v].len() == 4 || beside[v].len() < 3 {
            continue;
        }
        let corner = made.corners[v];
        // (The normal: of the fan of the control points, in the order
        // the faces go around.)
        let mut normal = Point3::ZERO;
        for ring in &poly.faces {
            if let Some(k) = ring.iter().position(|&x| x == v) {
                let (next, prev) = (ring[(k + 1) % 4], ring[(k + 3) % 4]);
                normal += (made.along[&(v, next)] - corner).cross(made.along[&(v, prev)] - corner);
            }
        }
        let Some(normal) = normal.try_normalize() else {
            continue;
        };
        for &w in &beside[v] {
            if let Some(p) = made.along.get_mut(&(v, w)) {
                *p -= normal * (*p - corner).dot(normal);
            }
        }
    }
    for ring in &poly.faces {
        let mut inside = [Point3::ZERO; 4];
        for (k, point) in inside.iter_mut().enumerate() {
            let (v, next, far, prev) = (
                ring[k],
                ring[(k + 1) % 4],
                ring[(k + 2) % 4],
                ring[(k + 3) % 4],
            );
            let c = count(v);
            *point = (c * at(v) + 2.0 * at(next) + 2.0 * at(prev) + at(far)) / (c + 5.0);
        }
        made.inside.push(inside);
    }
    made
}

/// The point of the bicubic Bézier patch `grid` at (`u`, `v`).
pub fn point_on(grid: &[[Point3; 4]; 4], u: f64, v: f64) -> Point3 {
    let basis = |t: f64| {
        let s = 1.0 - t;
        [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t]
    };
    let (bu, bv) = (basis(u), basis(v));
    let mut p = Point3::ZERO;
    for i in 0..4 {
        for j in 0..4 {
            p += grid[i][j] * bu[i] * bv[j];
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prims::Primitive;

    #[test]
    fn a_step_makes_quads_and_keeps_a_closed_mesh_closed() {
        let cone = Primitive::Cone.mesh(2.0, 6, Point3::ZERO);
        let finer = refined(&cone);
        assert!(finer.faces.iter().all(|ring| ring.len() == 4));
        assert!(finer.edges().values().all(|s| s.len() == 2));
        // 6 triangles and a hexagon: 3 quads of each triangle, 6 of
        // the hexagon.
        assert_eq!(finer.faces.len(), 6 * 3 + 6);
    }

    #[test]
    fn the_patches_of_a_flat_grid_are_flat_and_end_at_its_rim() {
        let grid = Primitive::Grid.mesh(4.0, 4, Point3::ZERO);
        let patches = patches(&grid);
        // The corners of the grid are no corners of the surface: its
        // rim is the smooth curve the rim's points control.
        for (f, ring) in grid.faces.iter().enumerate() {
            let patch = patches.grid(f, ring);
            for (u, v) in [(0.0, 0.0), (0.3, 0.8), (1.0, 1.0)] {
                assert!(point_on(&patch, u, v).z.abs() < 1e-12);
            }
        }
        // In the middle the surface is where the mesh is.
        let middle = grid.points.iter().position(|p| p.length() < 1e-12).unwrap();
        assert!(patches.corners[middle].length() < 1e-12);
    }

    #[test]
    fn patches_beside_each_other_share_their_sides() {
        let ball = refined(&Primitive::Box.mesh(2.0, 1, Point3::ZERO));
        let patches = patches(&ball);
        for (f, ring) in ball.faces.iter().enumerate() {
            let patch = patches.grid(f, ring);
            // The side from the first corner to the second is the
            // curve of that edge, whichever face asks.
            let on_side = point_on(&patch, 0.25, 0.0);
            let (a, b) = (ring[0], ring[1]);
            let curve = [
                patches.corners[a],
                patches.along[&(a, b)],
                patches.along[&(b, a)],
                patches.corners[b],
            ];
            let t: f64 = 0.25;
            let s = 1.0 - t;
            let on_curve = curve[0] * s * s * s
                + curve[1] * 3.0 * s * s * t
                + curve[2] * 3.0 * s * t * t
                + curve[3] * t * t * t;
            assert!(on_side.distance(on_curve) < 1e-12);
        }
        // A box goes towards something round: every point of the
        // surface about as far from the middle.
        let far: Vec<f64> = patches.corners.iter().map(|p| p.length()).collect();
        let (least, most) = far
            .iter()
            .fold((f64::MAX, 0.0_f64), |(lo, hi), &d| (lo.min(d), hi.max(d)));
        assert!(most / least < 1.25, "{least} to {most}");
    }
}
