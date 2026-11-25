//! What of a world is shown: everything but the datums and bodies the user
//! (or the UI) hid, and never what is only there for the parametrisation:
//! seams, where a surface's parameters wrap, and vertices that only close
//! a loop (where a circle starts). Drawing and picking both follow it.

use std::collections::{BTreeMap, BTreeSet};

use super::World;
use super::topo::{CoedgeKind, EdgeId, FaceId, NurbsSurface, Topo, VertexId};
use crate::math::Point3;

/// The datums, bodies and instances hidden, by name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Visibility {
    /// Hidden datums.
    pub datums: BTreeSet<String>,
    /// Hidden bodies (by the body's own name).
    pub bodies: BTreeSet<String>,
    /// Hidden instances.
    pub instances: BTreeSet<String>,
    /// Hidden profiles (sketches).
    pub profiles: BTreeSet<String>,
    /// How the materials show, by name: a body or a face shows as
    /// what it is made of does (see [`World::face_look`]).
    pub shades: super::Shades,
    /// The size datums are drawn (and picked) at, where that is set and
    /// not the world's own (see [`World::datum_size`]).
    pub datum_size: Option<f64>,
    /// Whether sketches are drawn in front of everything: while one is
    /// to be picked.
    pub sketches_in_front: bool,
    /// Whether datums are drawn in front of everything: while one is to
    /// be picked.
    pub datums_in_front: bool,
}

impl Visibility {
    /// What shows of a part placed in another: its bodies and what it
    /// places, not the datums and sketches it was built from; its
    /// materials show as `shades` has them.
    pub fn placed(world: &World, shades: &super::Shades) -> Self {
        Self {
            datums: world.datums.keys().cloned().collect(),
            profiles: world.profiles.keys().cloned().collect(),
            shades: shades.clone(),
            datum_size: None,
            sketches_in_front: false,
            datums_in_front: false,
            ..Self::default()
        }
    }

    /// Everything shown.
    pub const ALL: Self = Self {
        datums: BTreeSet::new(),
        bodies: BTreeSet::new(),
        instances: BTreeSet::new(),
        shades: BTreeMap::new(),
        datum_size: None,
        sketches_in_front: false,
        datums_in_front: false,
        profiles: BTreeSet::new(),
    };

    /// Everything `world` has: hiding it leaves what another world adds
    /// to it.
    pub fn of(world: &World) -> Self {
        let topo = &world.topo;
        Self {
            datums: world.datums.keys().cloned().collect(),
            bodies: topo
                .bodies()
                .ids()
                .filter_map(|b| topo.name_of(b))
                .map(str::to_owned)
                .collect(),
            instances: world.instances.keys().cloned().collect(),
            profiles: world.profiles.keys().cloned().collect(),
            ..Self::default()
        }
    }
}

/// The topology that is shown.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shown {
    /// Faces.
    pub faces: Vec<FaceId>,
    /// Edges, without seams.
    pub edges: Vec<EdgeId>,
    /// The vertices that end an open one of those edges: corners, not
    /// where a closed edge starts or seams meet it.
    pub vertices: Vec<VertexId>,
}

/// The faces, edges and vertices of `topo` shown under `visibility`.
pub fn shown_topology(topo: &Topo, visibility: &Visibility) -> Shown {
    // What hidden bodies are made of.
    let mut hidden_faces = BTreeSet::new();
    for name in &visibility.bodies {
        if let Some(faces) = topo.resolve(name).and_then(|b| topo.body_faces(b)) {
            hidden_faces.extend(faces);
        }
    }
    let mut seams = BTreeSet::new();
    let mut hidden_edges = BTreeSet::new();
    // (A hidden curve's edges are in no face.)
    for name in &visibility.bodies {
        if let Some(body) = topo.resolve(name).and_then(|b| topo.body(b)) {
            hidden_edges.extend(body.free_edges.iter().copied());
        }
    }
    let mut faces = Vec::new();
    // Of each edge, the planes of the flat faces along it.
    let mut beside: BTreeMap<EdgeId, Vec<(Point3, f64, f64)>> = BTreeMap::new();
    for (id, face) in topo.faces().iter() {
        let plane = topo
            .surface(face.surface)
            .and_then(flat)
            .map(|(normal, offset)| (normal, offset, face.tolerance));
        let mut uses: BTreeMap<EdgeId, usize> = BTreeMap::new();
        for coedge in face
            .loops
            .iter()
            .filter_map(|&l| topo.loop_(l))
            .flat_map(|l| &l.coedges)
            .filter_map(|&c| topo.coedge(c))
        {
            if let CoedgeKind::Edge { edge, .. } = coedge.kind {
                *uses.entry(edge).or_default() += 1;
            }
        }
        seams.extend(uses.iter().filter(|&(_, &n)| n > 1).map(|(&e, _)| e));
        if let Some(plane) = plane {
            for &edge in uses.keys() {
                beside.entry(edge).or_default().push(plane);
            }
        }
        if hidden_faces.contains(&id) {
            hidden_edges.extend(uses.into_keys());
        } else {
            faces.push(id);
        }
    }
    // An edge between two faces in one plane is no edge to look at: two
    // blocks set flush side by side show one flat top.
    let level = |e: &EdgeId| {
        beside.get(e).is_some_and(|planes| match planes[..] {
            [(n, d, s), (m, e, t)] => {
                let same = n.dot(m);
                same.abs() > 1.0 - 1e-12
                    && (d - e * same.signum()).abs() <= s + t + 1e-9 * (1.0 + d.abs())
            }
            _ => false,
        })
    };
    let edges: Vec<EdgeId> = topo
        .edges()
        .ids()
        .filter(|e| !seams.contains(e) && !hidden_edges.contains(e) && !level(e))
        .collect();
    // The ends of the open ones — but not where just two of them meet
    // and run straight on: that is the middle of an edge, as far as
    // anyone can see.
    let mut ends: BTreeMap<VertexId, Vec<EdgeId>> = BTreeMap::new();
    for &id in &edges {
        if let Some(edge) = topo.edge(id).filter(|e| !e.is_closed()) {
            for v in [edge.start, edge.end] {
                ends.entry(v).or_default().push(id);
            }
        }
    }
    let straight_on = |v: VertexId, edges: &[EdgeId]| {
        let &[a, b] = edges else {
            return false;
        };
        let away = |id: EdgeId| {
            let edge = topo.edge(id)?;
            let curve = topo.curve(edge.curve)?;
            let line = curve.degree() == 1 && curve.control_points().len() == 2;
            let (near, far) = if edge.start == v {
                (edge.t0, edge.t1)
            } else {
                (edge.t1, edge.t0)
            };
            line.then(|| (curve.point_at(far) - curve.point_at(near)).try_normalize())?
        };
        match (away(a), away(b)) {
            (Some(x), Some(y)) => x.dot(y) < -1.0 + 1e-12,
            _ => false,
        }
    };
    let vertices: Vec<VertexId> = ends
        .iter()
        .filter(|(v, edges)| !straight_on(**v, edges))
        .map(|(v, _)| *v)
        .collect();
    Shown {
        faces,
        edges,
        vertices,
    }
}

/// The plane of a flat patch: its unit normal, and how far along it the
/// plane is from the origin.
fn flat(surface: &NurbsSurface) -> Option<(Point3, f64)> {
    if surface.degrees() != (1, 1) || surface.counts() != (2, 2) {
        return None;
    }
    let [p00, p10, p01, p11] =
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(i, j)| surface.control_point(i, j));
    let normal = (p10 - p00).cross(p01 - p00).try_normalize()?;
    let size = (p10 - p00).length() + (p01 - p00).length();
    ((p11 - p00).dot(normal).abs() <= 1e-12 * size).then(|| (normal, p00.dot(normal)))
}

impl World {
    /// The instances shown under `visibility`, by name.
    pub fn shown_instances<'a>(
        &'a self,
        visibility: &'a Visibility,
    ) -> impl Iterator<Item = (&'a String, &'a super::Instance)> {
        self.instances
            .iter()
            .filter(|(name, _)| !visibility.instances.contains(*name))
    }

    /// The profiles shown under `visibility`, by name.
    pub fn shown_profiles<'a>(
        &'a self,
        visibility: &'a Visibility,
    ) -> impl Iterator<Item = (&'a String, &'a super::Profile)> {
        self.profiles
            .iter()
            .filter(|(name, _)| !visibility.profiles.contains(*name))
    }

    /// The datums shown under `visibility`, by name.
    pub fn shown_datums<'a>(
        &'a self,
        visibility: &'a Visibility,
    ) -> impl Iterator<Item = (&'a String, &'a super::Datum)> {
        self.datums
            .iter()
            .filter(|(name, _)| !visibility.datums.contains(*name))
    }
}
