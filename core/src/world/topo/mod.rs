//! A boundary representation over NURBS geometry, with persistent names.
//!
//! # Why this structure
//!
//! The structure is an arena with typed ids, oriented coedges with pcurves,
//! seams, sheets and solids, and multiple lumps. Names come from operation
//! provenance, and a lost name is an error, never a guess.
//!
//! One deliberate departure: many kernels model poles as degenerate
//! *edges* without a 3D curve. Here a pole is a [`CoedgeKind::Pole`]
//! coedge instead — the collapse belongs to one face's parametrisation, not
//! to anything shared in 3D — so every edge is a real curve with length.
//!
//! * **Arena + typed ids** ([`arena`]): one `Arena` per
//!   entity kind, no `Rc`/`RefCell`, cheap to clone, deterministic
//!   iteration; slots are never reused, so stale ids fail to resolve.
//! * **Coedge-based B-rep** ([`entity`]): Body → Lump → Shell → Face →
//!   Loop → Coedge → Edge → Vertex. Orientation lives on the uses
//!   (coedge, face use), so edges and faces are shared without copying.
//! * **NURBS only** ([`nurbs`]): every edge curve, pcurve and surface is a
//!   (rational) B-spline. No analytic types.
//! * **Names** ([`naming`]): a bijection `EntityId ↔ String` kept beside
//!   the arenas and pruned when an entity is removed.
//!
//! Tolerances: every vertex, edge and face carries one radius
//! (`tolerance: f64`): a ball around a vertex, a tube around an edge, a
//! slab around a face.
//!
//! Inserts always check what they reference (ids exist, loops close,
//! ranges lie in the curve domain), so the store never holds a dangling
//! id. In debug builds they also run the geometric and topological
//! validity rules of [`validation`]; [`Topo::validate`] runs all of them on
//! demand. Entities are immutable once inserted.

pub mod arena;
pub mod entity;
pub mod naming;
pub mod nurbs;
pub mod validation;

pub use arena::{Arena, Id};
pub use entity::*;
pub use naming::{NameMap, NamingError};
pub use nurbs::{ControlPoint, Curve2, Curve3, NurbsCurve, NurbsError, NurbsSurface};
pub use validation::ValidationError;

/// Why the topology refused a change.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TopoError {
    /// A referenced id does not resolve.
    #[error("{0:?} does not exist")]
    Missing(EntityId),
    /// A point is not finite.
    #[error("point is not finite")]
    NonFinitePoint,
    /// An edge or pole range is empty or not finite.
    #[error("range [{t0}, {t1}] is empty or not finite")]
    BadRange {
        /// Start parameter.
        t0: f64,
        /// End parameter.
        t1: f64,
    },
    /// An edge range leaves its curve's domain.
    #[error("range {range:?} leaves the domain {domain:?} of {curve:?}")]
    RangeOutsideDomain {
        /// The curve.
        curve: CurveId,
        /// Requested range.
        range: (f64, f64),
        /// The curve's domain.
        domain: (f64, f64),
    },
    /// A loop has no coedges.
    #[error("a loop needs at least one coedge")]
    EmptyLoop,
    /// Coedge `index` of a loop does not end where the next one starts.
    #[error("loop is open after coedge {index}")]
    OpenLoop {
        /// Position in the loop of the coedge whose end does not match.
        index: usize,
    },
    /// A lump has no shells.
    #[error("a lump needs at least one shell")]
    EmptyLump,
    /// The body's contents contradict its kind.
    #[error("contents do not match body kind {0:?}")]
    KindMismatch(BodyKind),
    /// The entity is still used by another and cannot be removed.
    #[error("{id:?} is still used by {by:?}")]
    StillReferenced {
        /// What was to be removed.
        id: EntityId,
        /// One entity that uses it.
        by: EntityId,
    },
    /// A naming conflict.
    #[error(transparent)]
    Naming(#[from] NamingError),
    /// The entity breaks a validity rule (checked in debug builds only).
    #[error(transparent)]
    Invalid(#[from] ValidationError),
}

/// The B-rep store: one arena per entity kind plus the name map.
#[derive(Debug, Clone, Default)]
pub struct Topo {
    vertices: Arena<Vertex>,
    edges: Arena<Edge>,
    coedges: Arena<Coedge>,
    loops: Arena<Loop>,
    faces: Arena<Face>,
    shells: Arena<Shell>,
    lumps: Arena<Lump>,
    bodies: Arena<Body>,
    curves: Arena<Curve3>,
    pcurves: Arena<Curve2>,
    surfaces: Arena<NurbsSurface>,
    names: NameMap,
    /// The names of the bodies written — named, renamed or removed —
    /// since they were last asked for (see [`Self::take_written`]).
    written: std::collections::BTreeSet<String>,
    /// What was read of it while that was watched (see [`Self::watch`]).
    seen: Seen,
}

/// Something read of a topology while it was watched.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Looked {
    /// The body of this name: it, or its faces.
    Body(String),
    /// What this name refers to.
    Named(String),
    /// Which bodies there are.
    Listing,
    /// Anything of any body.
    All,
}

/// What is read of a topology, noted while it is watched.
#[derive(Debug, Default)]
struct Seen {
    on: std::sync::atomic::AtomicBool,
    what: std::sync::Mutex<std::collections::BTreeSet<Looked>>,
}

impl Clone for Seen {
    /// (A copy is watched as what it is a copy of is, and has been
    /// read nothing of.)
    fn clone(&self) -> Self {
        let on = self.on.load(std::sync::atomic::Ordering::Relaxed);
        Self {
            on: on.into(),
            what: Default::default(),
        }
    }
}

impl Seen {
    fn note(&self, looked: Looked) {
        let mut what = self
            .what
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        what.insert(looked);
    }

    fn watched(&self) -> bool {
        self.on.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn one(&self, names: &NameMap, id: EntityId) {
        if let EntityId::Body(_) = id
            && self.watched()
        {
            self.note(match names.name_of(id) {
                Some(name) => Looked::Body(name.to_owned()),
                None => Looked::All,
            });
        }
    }

    fn all(&self, of: &str) {
        if self.watched() {
            self.note(if of == "bodies" {
                Looked::Listing
            } else {
                Looked::All
            });
        }
    }

    fn named(&self, name: &str) {
        if self.watched() {
            self.note(Looked::Named(name.to_owned()));
        }
    }
}

macro_rules! accessors {
    ($( $field:ident : $ty:ty, $id:ty, $get:ident, $all:ident; )*) => {$(
        #[doc = concat!("The `", stringify!($ty), "` behind `id`.")]
        pub fn $get(&self, id: $id) -> Option<&$ty> {
            self.seen.one(&self.names, id.into());
            self.$field.get(id)
        }

        #[doc = concat!("Every `", stringify!($ty), "`, in id order.")]
        pub fn $all(&self) -> &Arena<$ty> {
            self.seen.all(stringify!($field));
            &self.$field
        }
    )*};
}

impl Topo {
    /// An empty topology.
    pub fn new() -> Self {
        Self::default()
    }

    accessors! {
        vertices: Vertex, VertexId, vertex, vertices;
        edges: Edge, EdgeId, edge, edges;
        coedges: Coedge, CoedgeId, coedge, coedges;
        loops: Loop, LoopId, loop_, loops;
        faces: Face, FaceId, face, faces;
        shells: Shell, ShellId, shell, shells;
        lumps: Lump, LumpId, lump, lumps;
        bodies: Body, BodyId, body, bodies;
        curves: Curve3, CurveId, curve, curves;
        pcurves: Curve2, PCurveId, pcurve, pcurves;
        surfaces: NurbsSurface, SurfaceId, surface, surfaces;
    }

    /// `true` when `id` resolves.
    pub fn contains(&self, id: impl Into<EntityId>) -> bool {
        match id.into() {
            EntityId::Vertex(i) => self.vertices.contains(i),
            EntityId::Edge(i) => self.edges.contains(i),
            EntityId::Coedge(i) => self.coedges.contains(i),
            EntityId::Loop(i) => self.loops.contains(i),
            EntityId::Face(i) => self.faces.contains(i),
            EntityId::Shell(i) => self.shells.contains(i),
            EntityId::Lump(i) => self.lumps.contains(i),
            EntityId::Body(i) => self.bodies.contains(i),
            EntityId::Curve(i) => self.curves.contains(i),
            EntityId::PCurve(i) => self.pcurves.contains(i),
            EntityId::Surface(i) => self.surfaces.contains(i),
        }
    }

    fn require(&self, id: impl Into<EntityId>) -> Result<(), TopoError> {
        let id = id.into();
        if self.contains(id) {
            Ok(())
        } else {
            Err(TopoError::Missing(id))
        }
    }

    // ---- geometry -------------------------------------------------------

    /// Stores a model-space curve.
    pub fn add_curve(&mut self, curve: Curve3) -> CurveId {
        self.curves.insert(curve)
    }

    /// Stores a parameter-space curve.
    pub fn add_pcurve(&mut self, pcurve: Curve2) -> PCurveId {
        self.pcurves.insert(pcurve)
    }

    /// Stores a surface.
    pub fn add_surface(&mut self, surface: NurbsSurface) -> SurfaceId {
        self.surfaces.insert(surface)
    }

    // ---- topology -------------------------------------------------------

    /// Adds a vertex.
    pub fn add_vertex(&mut self, vertex: Vertex) -> Result<VertexId, TopoError> {
        if !vertex.point.is_finite() {
            return Err(TopoError::NonFinitePoint);
        }
        let id = self.vertices.insert(vertex);
        self.debug_check(id.into())?;
        Ok(id)
    }

    /// Adds an edge; its vertices and curve must exist and its range must
    /// be a non-empty part of the curve's domain.
    pub fn add_edge(&mut self, edge: Edge) -> Result<EdgeId, TopoError> {
        self.require(edge.start)?;
        self.require(edge.end)?;
        let (t0, t1) = (edge.t0, edge.t1);
        check_range(t0, t1)?;
        let curve = self
            .curves
            .get(edge.curve)
            .ok_or(TopoError::Missing(edge.curve.into()))?;
        let domain = curve.domain();
        if t0 < domain.0 || t1 > domain.1 {
            return Err(TopoError::RangeOutsideDomain {
                curve: edge.curve,
                range: (t0, t1),
                domain,
            });
        }
        let id = self.edges.insert(edge);
        self.debug_check(id.into())?;
        Ok(id)
    }

    /// Adds a coedge; its edge (or pole vertex) and pcurve must exist, and
    /// a pole's range must be non-empty.
    pub fn add_coedge(&mut self, coedge: Coedge) -> Result<CoedgeId, TopoError> {
        match coedge.kind {
            CoedgeKind::Edge { edge, .. } => self.require(edge)?,
            CoedgeKind::Pole { vertex, t0, t1 } => {
                self.require(vertex)?;
                check_range(t0, t1)?;
            }
        }
        if let Some(p) = coedge.pcurve {
            self.require(p)?;
        }
        let id = self.coedges.insert(coedge);
        self.debug_check(id.into())?;
        Ok(id)
    }

    /// Adds a loop; it must be non-empty and close up: each coedge ends at
    /// the vertex the next one starts at, cyclically.
    pub fn add_loop(&mut self, loop_: Loop) -> Result<LoopId, TopoError> {
        if loop_.coedges.is_empty() {
            return Err(TopoError::EmptyLoop);
        }
        let mut ends = Vec::with_capacity(loop_.coedges.len());
        for &c in &loop_.coedges {
            let co = self.coedges.get(c).ok_or(TopoError::Missing(c.into()))?;
            ends.push(self.coedge_ends(co));
        }
        for i in 0..ends.len() {
            if ends[i].1 != ends[(i + 1) % ends.len()].0 {
                return Err(TopoError::OpenLoop { index: i });
            }
        }
        let id = self.loops.insert(loop_);
        self.debug_check(id.into())?;
        Ok(id)
    }

    /// Adds a face; its surface and loops must exist.
    pub fn add_face(&mut self, face: Face) -> Result<FaceId, TopoError> {
        self.require(face.surface)?;
        for &l in &face.loops {
            self.require(l)?;
        }
        let id = self.faces.insert(face);
        self.debug_check(id.into())?;
        Ok(id)
    }

    /// Adds a shell; its faces must exist.
    pub fn add_shell(&mut self, shell: Shell) -> Result<ShellId, TopoError> {
        for u in &shell.faces {
            self.require(u.face)?;
        }
        let id = self.shells.insert(shell);
        self.debug_check(id.into())?;
        Ok(id)
    }

    /// Adds a lump; it needs at least one shell, all existing.
    pub fn add_lump(&mut self, lump: Lump) -> Result<LumpId, TopoError> {
        if lump.shells.is_empty() {
            return Err(TopoError::EmptyLump);
        }
        for &s in &lump.shells {
            self.require(s)?;
        }
        let id = self.lumps.insert(lump);
        self.debug_check(id.into())?;
        Ok(id)
    }

    /// Adds a body; its parts must exist and fit its kind (a wire has no
    /// lumps; solids and sheets have lumps and nothing free).
    pub fn add_body(&mut self, body: Body) -> Result<BodyId, TopoError> {
        for &l in &body.lumps {
            self.require(l)?;
        }
        for &e in &body.free_edges {
            self.require(e)?;
        }
        for &v in &body.free_vertices {
            self.require(v)?;
        }
        let free = !body.free_edges.is_empty() || !body.free_vertices.is_empty();
        let ok = match body.kind {
            BodyKind::Solid | BodyKind::Sheet => !body.lumps.is_empty() && !free,
            BodyKind::Wire => body.lumps.is_empty() && !body.free_edges.is_empty(),
            BodyKind::General => true,
        };
        if !ok {
            return Err(TopoError::KindMismatch(body.kind));
        }
        let id = self.bodies.insert(body);
        self.debug_check(id.into())?;
        Ok(id)
    }

    // ---- removal --------------------------------------------------------

    /// One entity that refers to `id`, if any.
    pub fn referrer(&self, id: impl Into<EntityId>) -> Option<EntityId> {
        let id = id.into();
        match id {
            EntityId::Vertex(v) => self
                .edges
                .iter()
                .find(|(_, e)| e.start == v || e.end == v)
                .map(|(i, _)| i.into())
                .or_else(|| {
                    self.coedges
                        .iter()
                        .find(|(_, c)| matches!(c.kind, CoedgeKind::Pole { vertex, .. } if vertex == v))
                        .map(|(i, _)| i.into())
                })
                .or_else(|| {
                    self.bodies
                        .iter()
                        .find(|(_, b)| b.free_vertices.contains(&v))
                        .map(|(i, _)| i.into())
                }),
            EntityId::Edge(e) => self
                .coedges
                .iter()
                .find(|(_, c)| c.edge_id() == Some(e))
                .map(|(i, _)| i.into())
                .or_else(|| {
                    self.bodies
                        .iter()
                        .find(|(_, b)| b.free_edges.contains(&e))
                        .map(|(i, _)| i.into())
                }),
            EntityId::Coedge(c) => self
                .loops
                .iter()
                .find(|(_, l)| l.coedges.contains(&c))
                .map(|(i, _)| i.into()),
            EntityId::Loop(l) => self
                .faces
                .iter()
                .find(|(_, f)| f.loops.contains(&l))
                .map(|(i, _)| i.into()),
            EntityId::Face(f) => self
                .shells
                .iter()
                .find(|(_, s)| s.faces.iter().any(|u| u.face == f))
                .map(|(i, _)| i.into()),
            EntityId::Shell(s) => self
                .lumps
                .iter()
                .find(|(_, l)| l.shells.contains(&s))
                .map(|(i, _)| i.into()),
            EntityId::Lump(l) => self
                .bodies
                .iter()
                .find(|(_, b)| b.lumps.contains(&l))
                .map(|(i, _)| i.into()),
            EntityId::Body(_) => None,
            EntityId::Curve(c) => self
                .edges
                .iter()
                .find(|(_, e)| e.curve == c)
                .map(|(i, _)| i.into()),
            EntityId::PCurve(p) => self
                .coedges
                .iter()
                .find(|(_, c)| c.pcurve == Some(p))
                .map(|(i, _)| i.into()),
            EntityId::Surface(s) => self
                .faces
                .iter()
                .find(|(_, f)| f.surface == s)
                .map(|(i, _)| i.into()),
        }
    }

    /// Removes one entity and its name. Refused while anything still
    /// refers to it, so the store never holds a dangling id. (Linear scan;
    /// bulk deletion should go top-down: body first, vertices last.)
    pub fn remove(&mut self, id: impl Into<EntityId>) -> Result<(), TopoError> {
        let id = id.into();
        self.require(id)?;
        if let Some(by) = self.referrer(id) {
            return Err(TopoError::StillReferenced { id, by });
        }
        self.discard(id);
        Ok(())
    }

    /// Notes the body `id`, if it is one with a name, as written.
    fn note_written(&mut self, id: EntityId) {
        if let EntityId::Body(_) = id
            && let Some(name) = self.names.name_of(id)
        {
            self.written.insert(name.to_owned());
        }
    }

    /// The names of the bodies written since this was last asked: those
    /// given a name (a body is written whole, then named), renamed, or
    /// removed. What a step did to the bodies, without comparing them
    /// all; a step that changes a body under its name says so itself
    /// (see [`Self::wrote`]).
    pub fn take_written(&mut self) -> std::collections::BTreeSet<String> {
        std::mem::take(&mut self.written)
    }

    /// The body called `name`, if there is one. (Whether there is is
    /// no reading of any body.)
    pub fn body_called(&self, name: &str) -> Option<BodyId> {
        self.names
            .id_of(name)
            .and_then(|id| BodyId::try_from(id).ok())
    }

    /// The body that `id` is of (a body: itself). Which body something
    /// is of is no reading of the bodies it is not of.
    pub fn owner(&self, id: impl Into<EntityId>) -> Option<BodyId> {
        let id = id.into();
        if let EntityId::Body(body) = id {
            return self.bodies.contains(body).then_some(body);
        }
        let holds = |face: FaceId| -> bool {
            if id == EntityId::Face(face) {
                return true;
            }
            let Some(f) = self.faces.get(face) else {
                return false;
            };
            let coedges = f
                .loops
                .iter()
                .filter_map(|&l| self.loops.get(l))
                .flat_map(|l| &l.coedges);
            coedges
                .filter_map(|&c| self.coedges.get(c))
                .any(|c| match c.kind {
                    CoedgeKind::Edge { edge, .. } => {
                        id == EntityId::Edge(edge)
                            || self.edges.get(edge).is_some_and(|e| {
                                id == EntityId::Vertex(e.start) || id == EntityId::Vertex(e.end)
                            })
                    }
                    CoedgeKind::Pole { vertex, .. } => id == EntityId::Vertex(vertex),
                })
        };
        self.bodies.iter().map(|(b, _)| b).find(|&b| {
            let lumps = self.bodies.get(b).map_or(&[][..], |b| &b.lumps);
            let shells = lumps
                .iter()
                .filter_map(|&l| self.lumps.get(l))
                .flat_map(|l| &l.shells);
            let mut faces = shells
                .filter_map(|&s| self.shells.get(s))
                .flat_map(|s| &s.faces);
            faces.any(|u| holds(u.face))
        })
    }

    /// Starts noting what is read of it: which bodies, what by name,
    /// that all of them are gone through. What a step is applied to is
    /// watched, so that it is known what the step's result depends on.
    pub fn watch(&mut self) {
        *self
            .seen
            .what
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Default::default();
        *self.seen.on.get_mut() = true;
    }

    /// Stops noting what is read, for something to be written that is
    /// no reading of it; whether it was watched is returned, to
    /// [`resume`](Self::resume) with.
    pub fn pause(&mut self) -> bool {
        std::mem::replace(self.seen.on.get_mut(), false)
    }

    /// Watches again, if `watched`, what [`pause`](Self::pause) stopped.
    pub fn resume(&mut self, watched: bool) {
        *self.seen.on.get_mut() = watched;
    }

    /// What was read of it since it was watched, which it is no longer.
    pub fn watched(&mut self) -> Vec<Looked> {
        *self.seen.on.get_mut() = false;
        let what = self.seen.what.get_mut();
        let what = what.unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(what).into_iter().collect()
    }

    /// Says that the body called `name` was changed as it is.
    pub fn wrote(&mut self, name: &str) {
        self.written.insert(name.to_owned());
    }

    /// Removes the body `body` with everything only it is made of.
    pub fn remove_body(&mut self, body: BodyId) {
        // (Taking it out is no reading of it.)
        let watched = self.pause();
        self.remove_body_unwatched(body);
        self.resume(watched);
    }

    fn remove_body_unwatched(&mut self, body: BodyId) {
        let mut ids: Vec<EntityId> = vec![body.into()];
        let mut later: [Vec<EntityId>; 10] = Default::default();
        if let Some(b) = self.body(body) {
            for &l in &b.lumps {
                later[0].push(l.into());
                for &s in self.lump(l).map_or(&[][..], |l| &l.shells) {
                    later[1].push(s.into());
                    for u in self.shell(s).map_or(&[][..], |s| &s.faces) {
                        later[2].push(u.face.into());
                        let Some(face) = self.face(u.face) else {
                            continue;
                        };
                        later[9].push(face.surface.into());
                        for &l in &face.loops {
                            later[3].push(l.into());
                            for &c in self.loop_(l).map_or(&[][..], |l| &l.coedges) {
                                later[4].push(c.into());
                                let Some(coedge) = self.coedge(c) else {
                                    continue;
                                };
                                later[5].extend(coedge.pcurve.map(EntityId::from));
                                let (a, b) = self.coedge_ends(coedge);
                                later[8].extend::<[EntityId; 2]>([a.into(), b.into()]);
                                if let Some(e) = coedge.edge_id() {
                                    later[6].push(e.into());
                                    later[7].extend(self.edge(e).map(|e| EntityId::from(e.curve)));
                                }
                            }
                        }
                    }
                }
            }
        }
        ids.extend(later.into_iter().flatten());
        let mut seen = std::collections::BTreeSet::new();
        for id in ids {
            if seen.insert(id) {
                // What something else still uses stays.
                let _ = self.remove(id);
            }
        }
    }

    /// Drops `id` and its name without any check.
    fn discard(&mut self, id: EntityId) {
        self.note_written(id);
        match id {
            EntityId::Vertex(i) => drop(self.vertices.remove(i)),
            EntityId::Edge(i) => drop(self.edges.remove(i)),
            EntityId::Coedge(i) => drop(self.coedges.remove(i)),
            EntityId::Loop(i) => drop(self.loops.remove(i)),
            EntityId::Face(i) => drop(self.faces.remove(i)),
            EntityId::Shell(i) => drop(self.shells.remove(i)),
            EntityId::Lump(i) => drop(self.lumps.remove(i)),
            EntityId::Body(i) => drop(self.bodies.remove(i)),
            EntityId::Curve(i) => drop(self.curves.remove(i)),
            EntityId::PCurve(i) => drop(self.pcurves.remove(i)),
            EntityId::Surface(i) => drop(self.surfaces.remove(i)),
        }
        self.names.remove_id(id);
    }

    /// In debug builds, runs the validity checks of the just-inserted `id`
    /// and takes it out again when they fail. A no-op in release builds.
    fn debug_check(&mut self, id: EntityId) -> Result<(), TopoError> {
        if cfg!(debug_assertions)
            && let Err(e) = validation::check(self, id)
        {
            self.discard(id);
            return Err(e.into());
        }
        Ok(())
    }

    /// Runs every validity check on every entity, in any build, and
    /// returns all violations found.
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        validation::check_all(self)
    }

    // ---- names ----------------------------------------------------------

    /// The name map (read-only; change it through [`Self::set_name`],
    /// [`Self::rename`] and [`Self::clear_name`] so ids are checked).
    pub fn names(&self) -> &NameMap {
        self.seen.all("names");
        &self.names
    }

    /// Names an existing entity. Refused if the name is taken or the
    /// entity already has another name.
    pub fn set_name(
        &mut self,
        id: impl Into<EntityId>,
        name: impl Into<String>,
    ) -> Result<(), TopoError> {
        let id = id.into();
        self.require(id)?;
        self.names.insert(id, name)?;
        self.note_written(id);
        Ok(())
    }

    /// Renames an existing entity (or names it for the first time).
    pub fn rename(
        &mut self,
        id: impl Into<EntityId>,
        name: impl Into<String>,
    ) -> Result<(), TopoError> {
        let id = id.into();
        self.require(id)?;
        self.note_written(id);
        self.names.rename(id, name)?;
        self.note_written(id);
        Ok(())
    }

    /// Forgets the name of `id`.
    pub fn clear_name(&mut self, id: impl Into<EntityId>) -> Option<String> {
        let id = id.into();
        self.note_written(id);
        self.names.remove_id(id)
    }

    /// The name of `id`.
    pub fn name_of(&self, id: impl Into<EntityId>) -> Option<&str> {
        self.names.name_of(id)
    }

    /// The entity called `name`, of whatever kind.
    pub fn lookup(&self, name: &str) -> Option<EntityId> {
        self.seen.named(name);
        self.names.id_of(name)
    }

    /// Makes `id` also known by `name`: what a reference to something
    /// that was made one with it then means.
    pub fn also(&mut self, id: impl Into<EntityId>, name: impl Into<String>) {
        self.names.also(id, name);
    }

    /// The names `id` is known by besides its own.
    pub fn also_of(&self, id: impl Into<EntityId>) -> Vec<&str> {
        self.names.also_of(id)
    }

    /// What a reference to `name` means: the entity called so, or the
    /// one it was made one with — or, for a piece of a face that is in
    /// pieces no longer (`name#2`, once the face is whole again), the
    /// face.
    pub fn referred(&self, name: &str) -> Option<EntityId> {
        self.seen.named(name);
        self.names.known(name).or_else(|| {
            let (whole, piece) = name.rsplit_once('#')?;
            let numbered = !piece.is_empty() && piece.bytes().all(|b| b.is_ascii_digit());
            numbered.then(|| self.referred(whole))?
        })
    }

    /// The entity called `name`, if it is of kind `T`
    /// (e.g. `topo.resolve::<FaceId>("extrude1/cap/top")`).
    pub fn resolve<T: TryFrom<EntityId>>(&self, name: &str) -> Option<T> {
        self.referred(name).and_then(|id| T::try_from(id).ok())
    }

    // ---- adjacency ------------------------------------------------------

    /// Every coedge that uses `edge` (its radial uses), in id order.
    pub fn coedges_of_edge(&self, edge: EdgeId) -> Vec<CoedgeId> {
        self.coedges
            .iter()
            .filter(|(_, c)| c.edge_id() == Some(edge))
            .map(|(i, _)| i)
            .collect()
    }

    /// The vertices a coedge starts and ends at, in walking order (the
    /// same vertex twice for a pole).
    pub fn coedge_ends(&self, coedge: &Coedge) -> (VertexId, VertexId) {
        match coedge.kind {
            CoedgeKind::Edge { edge, orientation } => {
                let e = &self.edges[edge];
                match orientation {
                    Orientation::Forward => (e.start, e.end),
                    Orientation::Reversed => (e.end, e.start),
                }
            }
            CoedgeKind::Pole { vertex, .. } => (vertex, vertex),
        }
    }

    /// The parameter range a coedge's pcurve must cover: its edge's range,
    /// or the pole's own.
    pub fn coedge_range(&self, coedge: &Coedge) -> (f64, f64) {
        match coedge.kind {
            CoedgeKind::Edge { edge, .. } => {
                let e = &self.edges[edge];
                (e.t0, e.t1)
            }
            CoedgeKind::Pole { t0, t1, .. } => (t0, t1),
        }
    }

    /// The face that owns `loop_`, if any.
    pub fn face_of_loop(&self, loop_: LoopId) -> Option<FaceId> {
        self.faces
            .iter()
            .find(|(_, f)| f.loops.contains(&loop_))
            .map(|(i, _)| i)
    }

    /// The vertices of a loop in walking order (start vertex of each coedge).
    pub fn loop_vertices(&self, loop_: LoopId) -> Option<Vec<VertexId>> {
        let l = self.loops.get(loop_)?;
        Some(
            l.coedges
                .iter()
                .map(|&c| self.coedge_ends(&self.coedges[c]).0)
                .collect(),
        )
    }

    /// Whether the outside of the body `face` bounds is where the
    /// face's surface's own normal points (`None` for a face no shell
    /// uses).
    pub fn faces_outwards(&self, face: FaceId) -> Option<bool> {
        self.shells
            .iter()
            .flat_map(|(_, shell)| &shell.faces)
            .find(|u| u.face == face)
            .map(|u| u.orientation == Orientation::Forward)
    }

    /// Every face of a body, through lumps and shells, deduplicated, in
    /// first-seen order.
    pub fn body_faces(&self, body: BodyId) -> Option<Vec<FaceId>> {
        self.seen.one(&self.names, body.into());
        let b = self.bodies.get(body)?;
        let mut out = Vec::new();
        for &l in &b.lumps {
            for &s in &self.lumps[l].shells {
                for u in &self.shells[s].faces {
                    if !out.contains(&u.face) {
                        out.push(u.face);
                    }
                }
            }
        }
        Some(out)
    }
}

#[cfg(test)]
pub(crate) mod tests;

/// A parameter range must be finite and non-empty.
fn check_range(t0: f64, t1: f64) -> Result<(), TopoError> {
    if t0.is_finite() && t1.is_finite() && t0 < t1 {
        Ok(())
    } else {
        Err(TopoError::BadRange { t0, t1 })
    }
}
