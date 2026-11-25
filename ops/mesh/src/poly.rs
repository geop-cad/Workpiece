//! The polygon mesh: points, faces that are rings of them, and lines —
//! free edges, of no face. The edges of faces are what two points
//! beside each other in a face make; they have no record of their own,
//! but those that are creased are kept.

use std::collections::{BTreeMap, BTreeSet};

use workpiece_core::math::Point3;

/// An edge: its two points, the lower first.
pub type Edge = (usize, usize);

/// The edge between `a` and `b`.
pub fn edge(a: usize, b: usize) -> Edge {
    (a.min(b), a.max(b))
}

/// A vertex, an edge or a face of a mesh: what is picked in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Item {
    /// A point, by its index.
    Vertex(usize),
    /// An edge, by its points, the lower first.
    Edge(usize, usize),
    /// A face, by its index.
    Face(usize),
}

/// A polygon mesh.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Poly {
    /// The points.
    pub points: Vec<Point3>,
    /// The faces: each the points around it, counter-clockwise seen
    /// from outside.
    pub faces: Vec<Vec<usize>>,
    /// The edges that stay sharp when the mesh is subdivided.
    #[serde(default)]
    pub creases: BTreeSet<Edge>,
    /// The free edges: lines between two points that are the edge of
    /// no face. Those that run on from each other make a curve: the
    /// limit curve of their subdivision.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub lines: BTreeSet<Edge>,
    /// The points where the curve of the lines through them has a
    /// corner (a crease of a point): it runs into them and out again
    /// sharp.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub corners: BTreeSet<usize>,
}

/// Where an edge is walked: by face `face`, from its corner `corner`
/// to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Side {
    /// The face.
    pub face: usize,
    /// The corner of the face the edge starts at, walking the face.
    pub corner: usize,
}

impl Poly {
    /// The edges, each with the faces that walk it.
    pub fn edges(&self) -> BTreeMap<Edge, Vec<Side>> {
        let mut all: BTreeMap<Edge, Vec<Side>> = BTreeMap::new();
        for (face, ring) in self.faces.iter().enumerate() {
            for corner in 0..ring.len() {
                let key = edge(ring[corner], ring[(corner + 1) % ring.len()]);
                all.entry(key).or_default().push(Side { face, corner });
            }
        }
        all
    }

    /// The faces at each point.
    pub fn faces_at(&self) -> Vec<Vec<usize>> {
        let mut at = vec![Vec::new(); self.points.len()];
        for (f, ring) in self.faces.iter().enumerate() {
            for &v in ring {
                if !at[v].contains(&f) {
                    at[v].push(f);
                }
            }
        }
        at
    }

    /// The points beside each point, along an edge.
    pub fn beside(&self) -> Vec<BTreeSet<usize>> {
        let mut at = vec![BTreeSet::new(); self.points.len()];
        for ring in &self.faces {
            for k in 0..ring.len() {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                at[a].insert(b);
                at[b].insert(a);
            }
        }
        for &(a, b) in &self.lines {
            at[a].insert(b);
            at[b].insert(a);
        }
        at
    }

    /// The middle of face `f`.
    pub fn center(&self, f: usize) -> Point3 {
        let ring = &self.faces[f];
        ring.iter().map(|&v| self.points[v]).sum::<Point3>() / ring.len().max(1) as f64
    }

    /// The normal of face `f`, as long as twice its area (Newell's).
    pub fn area_normal(&self, f: usize) -> Point3 {
        let ring = &self.faces[f];
        let mut n = Point3::ZERO;
        for k in 0..ring.len() {
            let (a, b) = (
                self.points[ring[k]],
                self.points[ring[(k + 1) % ring.len()]],
            );
            n += a.cross(b);
        }
        n
    }

    /// The unit normal of face `f` (zero for a face without area).
    pub fn normal(&self, f: usize) -> Point3 {
        self.area_normal(f).normalize_or_zero()
    }

    /// The unit normal at each point: of the faces around it, by area.
    pub fn point_normals(&self) -> Vec<Point3> {
        let mut normals = vec![Point3::ZERO; self.points.len()];
        for (f, ring) in self.faces.iter().enumerate() {
            let n = self.area_normal(f);
            for &v in ring {
                normals[v] += n;
            }
        }
        normals.into_iter().map(Point3::normalize_or_zero).collect()
    }

    /// The corners of a box around the points; none without points.
    pub fn extent(&self) -> Option<(Point3, Point3)> {
        let first = *self.points.first()?;
        Some(
            self.points
                .iter()
                .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))),
        )
    }

    /// How large the mesh is: the diagonal of the box around it, and
    /// one for a mesh of no size.
    pub fn size(&self) -> f64 {
        let size = self.extent().map_or(0.0, |(lo, hi)| lo.distance(hi));
        if size > 0.0 { size } else { 1.0 }
    }

    /// The mean length of the edges (and lines); a tenth of the size
    /// for none.
    pub fn mean_edge(&self) -> f64 {
        let edges = self.edges();
        let all: Vec<Edge> = edges.keys().chain(&self.lines).copied().collect();
        if all.is_empty() {
            return self.size() / 10.0;
        }
        let long: f64 = all
            .iter()
            .map(|&(a, b)| self.points[a].distance(self.points[b]))
            .sum();
        long / all.len() as f64
    }

    /// The points `items` touch.
    pub fn points_of(&self, items: &[Item]) -> BTreeSet<usize> {
        let mut all = BTreeSet::new();
        for item in items {
            match *item {
                Item::Vertex(v) => {
                    all.insert(v);
                }
                Item::Edge(a, b) => {
                    all.extend([a, b]);
                }
                Item::Face(f) => {
                    if let Some(ring) = self.faces.get(f) {
                        all.extend(ring.iter().copied());
                    }
                }
            }
        }
        all.retain(|&v| v < self.points.len());
        all
    }

    /// The faces among `items`.
    pub fn faces_of(&self, items: &[Item]) -> BTreeSet<usize> {
        items
            .iter()
            .filter_map(|i| match *i {
                Item::Face(f) if f < self.faces.len() => Some(f),
                _ => None,
            })
            .collect()
    }

    /// The edges among `items` that the mesh has.
    pub fn edges_of(&self, items: &[Item]) -> BTreeSet<Edge> {
        let have = self.edges();
        items
            .iter()
            .filter_map(|i| match *i {
                Item::Edge(a, b) if have.contains_key(&edge(a, b)) => Some(edge(a, b)),
                _ => None,
            })
            .collect()
    }

    /// Whether the mesh has `item`.
    pub fn has(&self, item: &Item) -> bool {
        match *item {
            Item::Vertex(v) => v < self.points.len(),
            Item::Face(f) => f < self.faces.len(),
            Item::Edge(a, b) => {
                self.lines.contains(&edge(a, b))
                    || self.faces.iter().any(|ring| {
                        (0..ring.len())
                            .any(|k| edge(ring[k], ring[(k + 1) % ring.len()]) == edge(a, b))
                    })
            }
        }
    }

    /// The lines among `items`.
    pub fn lines_of(&self, items: &[Item]) -> BTreeSet<Edge> {
        items
            .iter()
            .filter_map(|i| match *i {
                Item::Edge(a, b) if self.lines.contains(&edge(a, b)) => Some(edge(a, b)),
                _ => None,
            })
            .collect()
    }

    /// All edges: of faces, and the lines.
    pub fn all_edges(&self) -> BTreeSet<Edge> {
        let mut all: BTreeSet<Edge> = self.edges().into_keys().collect();
        all.extend(&self.lines);
        all
    }

    /// The middle of `item`.
    pub fn middle(&self, item: &Item) -> Point3 {
        match *item {
            Item::Vertex(v) => self.points[v],
            Item::Edge(a, b) => (self.points[a] + self.points[b]) / 2.0,
            Item::Face(f) => self.center(f),
        }
    }

    /// The mesh without the points in `gone` and without the faces at
    /// them; and where each point that stays is now.
    pub fn without_points(&self, gone: &BTreeSet<usize>) -> (Poly, Vec<Option<usize>>) {
        let mut now = Vec::with_capacity(self.points.len());
        let mut points = Vec::new();
        for (v, p) in self.points.iter().enumerate() {
            if gone.contains(&v) {
                now.push(None);
            } else {
                now.push(Some(points.len()));
                points.push(*p);
            }
        }
        let faces = self
            .faces
            .iter()
            .filter_map(|ring| ring.iter().map(|&v| now[v]).collect::<Option<Vec<_>>>())
            .collect();
        let creases = self
            .creases
            .iter()
            .filter_map(|&(a, b)| Some(edge(now[a]?, now[b]?)))
            .collect();
        let lines = self
            .lines
            .iter()
            .filter_map(|&(a, b)| Some(edge(now[a]?, now[b]?)))
            .collect();
        let corners = self.corners.iter().filter_map(|&v| now[v]).collect();
        let mut made = Poly {
            points,
            faces,
            creases,
            lines,
            corners,
        };
        made.tidy();
        (made, now)
    }

    /// The mesh without the faces in `gone`, and without the points
    /// only they were at.
    pub fn without_faces(&self, gone: &BTreeSet<usize>) -> (Poly, Vec<Option<usize>>) {
        let mut used = vec![false; self.points.len()];
        let mut left = vec![false; self.points.len()];
        for (f, ring) in self.faces.iter().enumerate() {
            for &v in ring {
                used[v] = true;
                if !gone.contains(&f) {
                    left[v] = true;
                }
            }
        }
        for &(a, b) in &self.lines {
            left[a] = true;
            left[b] = true;
        }
        let mut kept = self.clone();
        kept.faces = self
            .faces
            .iter()
            .enumerate()
            .filter(|(f, _)| !gone.contains(f))
            .map(|(_, ring)| ring.clone())
            .collect();
        let orphans = (0..self.points.len())
            .filter(|&v| used[v] && !left[v])
            .collect();
        kept.without_points(&orphans)
    }

    /// Puts the mesh in order: faces lose points that are there twice
    /// running, faces of fewer than three points go, and so do creases
    /// along no edge; a line that is the edge of a face now is a line
    /// no more.
    pub fn tidy(&mut self) {
        for ring in &mut self.faces {
            ring.dedup();
            while ring.len() > 1 && ring.first() == ring.last() {
                ring.pop();
            }
            // (Out and straight back: a spur.)
            let mut k = 0;
            while ring.len() >= 3 && k < ring.len() {
                let n = ring.len();
                if ring[(k + n - 1) % n] == ring[(k + 1) % n] {
                    let (first, second) = (k.max((k + 1) % n), k.min((k + 1) % n));
                    ring.remove(first);
                    ring.remove(second);
                    k = 0;
                } else {
                    k += 1;
                }
            }
        }
        self.faces.retain(|ring| ring.len() >= 3);
        let have = self.edges();
        self.creases.retain(|e| have.contains_key(e));
        let points = self.points.len();
        self.lines
            .retain(|e| e.0 != e.1 && e.1 < points && !have.contains_key(e));
        self.corners.retain(|&v| v < points);
    }

    /// Adds the face around `ring`, turned to walk each edge it shares
    /// with a face that is there the other way, as beside each other
    /// faces do. Its index.
    pub fn add_face(&mut self, mut ring: Vec<usize>) -> usize {
        let mut walked: BTreeSet<(usize, usize)> = BTreeSet::new();
        for face in &self.faces {
            for k in 0..face.len() {
                walked.insert((face[k], face[(k + 1) % face.len()]));
            }
        }
        let n = ring.len();
        let (mut same, mut other) = (0, 0);
        for k in 0..n {
            let (a, b) = (ring[k], ring[(k + 1) % n]);
            same += usize::from(walked.contains(&(a, b)));
            other += usize::from(walked.contains(&(b, a)));
        }
        if same > other {
            ring.reverse();
        }
        self.faces.push(ring);
        self.faces.len() - 1
    }

    /// The runs of `edges`, each the points along it in order: closed
    /// (the bool) where it comes back to where it starts.
    pub fn runs(edges: &BTreeSet<Edge>) -> Vec<(Vec<usize>, bool)> {
        let mut beside: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for &(a, b) in edges {
            beside.entry(a).or_default().push(b);
            beside.entry(b).or_default().push(a);
        }
        let mut left: BTreeSet<Edge> = edges.clone();
        let mut runs = Vec::new();
        // From the ends first: what is left then is closed.
        let mut starts: Vec<usize> = beside
            .iter()
            .filter(|(_, n)| n.len() != 2)
            .map(|(&v, _)| v)
            .collect();
        starts.extend(beside.keys().copied());
        for start in starts {
            while let Some(&first) = beside[&start]
                .iter()
                .find(|&&n| left.contains(&edge(start, n)))
            {
                let mut run = vec![start, first];
                left.remove(&edge(start, first));
                let mut closed = false;
                loop {
                    let at = run[run.len() - 1];
                    if beside[&at].len() != 2 && at != start {
                        break;
                    }
                    let Some(&next) = beside[&at].iter().find(|&&n| left.contains(&edge(at, n)))
                    else {
                        break;
                    };
                    left.remove(&edge(at, next));
                    if next == start {
                        closed = true;
                        break;
                    }
                    run.push(next);
                }
                runs.push((run, closed));
            }
        }
        runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_are_found_open_and_closed() {
        let square: BTreeSet<Edge> = [(0, 1), (1, 2), (2, 3), (0, 3)].into();
        let runs = Poly::runs(&square);
        assert_eq!(runs.len(), 1);
        assert!(runs[0].1 && runs[0].0.len() == 4);
        let chain: BTreeSet<Edge> = [(0, 1), (1, 2), (5, 6)].into();
        let runs = Poly::runs(&chain);
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|(_, closed)| !closed));
    }
}
