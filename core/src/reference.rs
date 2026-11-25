//! References: stable, name-based pointers into a [`World`], typed by what
//! they resolve to.
//!
//! A reference stores only *where* it points, as a [`Source`]; it never
//! stores arena ids, so it survives rebuilds. The type parameter says what
//! it must resolve to: a `Reference<DatumPlane>` accepts a datum plane, a
//! frame's plane, or a planar face; a `Reference<DatumAxis>` a datum axis,
//! a frame's axis, or a linear edge.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use crate::math::{Axis, Point3};
use crate::world::topo::{BodyId, EdgeId, EntityId, FaceId, VertexId};
use crate::world::{Datum, DatumAxis, DatumFrame, DatumPlane, World};

/// Where a reference points.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Source {
    /// A key of [`World::datums`].
    Datum(String),
    /// One axis of a datum frame: as an axis, that axis; as a plane, the
    /// frame's plane normal to it.
    FrameAxis {
        /// The frame's datum name.
        frame: String,
        /// Which of its axes.
        axis: Axis,
    },
    /// A persistent name in [`crate::world::Topo::names`].
    Topology(String),
    /// A key of [`World::profiles`]: a sketch.
    Profile(String),
    /// Something of a part placed in the world: `source` in the world of
    /// the instance `instance`, seen where that instance is. What it
    /// gives moves with the part.
    Placed {
        /// The instance's name.
        instance: String,
        /// What it is, in the instance's world.
        source: Box<Source>,
    },
}

impl Source {
    /// The name it points at: a datum's, a frame's, a topological
    /// entity's.
    pub fn name(&self) -> &str {
        match self {
            Source::Datum(name) | Source::Topology(name) | Source::Profile(name) => name,
            Source::FrameAxis { frame, .. } => frame,
            // The step that places the part is what makes it.
            Source::Placed { instance, .. } => instance,
        }
    }

    /// `source` of the instance at `instance` (names from the world
    /// down): itself, if that is the world's own.
    pub fn placed(instance: &[String], source: Source) -> Source {
        instance
            .iter()
            .rev()
            .fold(source, |source, name| Source::Placed {
                instance: name.clone(),
                source: Box::new(source),
            })
    }

    /// The instance it is in, if it is something of a placed part.
    pub fn instance(&self) -> Option<&str> {
        match self {
            Source::Placed { instance, .. } => Some(instance),
            _ => None,
        }
    }

    /// Whether it points at something the operation `id` makes: what is
    /// called `id`, or `"{id}/…"`.
    pub fn made_by(&self, id: &str) -> bool {
        self.name()
            .strip_prefix(id)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    }

    /// Follows the operation `from` being renamed `to`: if it points at
    /// something `from` makes, it points at what `to` makes instead.
    pub fn rename(&mut self, from: &str, to: &str) {
        if !self.made_by(from) {
            return;
        }
        let name = match self {
            Source::Datum(name) | Source::Topology(name) | Source::Profile(name) => name,
            Source::FrameAxis { frame, .. } => frame,
            Source::Placed { instance, .. } => instance,
        };
        *name = format!("{to}{}", &name[from.len()..]);
    }
}

/// A reference that resolves to a `V` (see [`Resolve`]).
pub struct Reference<V> {
    /// Where it points.
    pub source: Source,
    kind: PhantomData<fn() -> V>,
}

impl<V> Reference<V> {
    /// A reference to `source`. Whether it resolves to a `V` is checked on
    /// [`World::resolve`].
    pub fn new(source: Source) -> Self {
        Self {
            source,
            kind: PhantomData,
        }
    }

    /// A reference to a datum.
    pub fn datum(name: impl Into<String>) -> Self {
        Self::new(Source::Datum(name.into()))
    }

    /// A reference to an axis of a datum frame.
    pub fn frame_axis(frame: impl Into<String>, axis: Axis) -> Self {
        Self::new(Source::FrameAxis {
            frame: frame.into(),
            axis,
        })
    }

    /// A reference to a named topological entity.
    pub fn topology(name: impl Into<String>) -> Self {
        Self::new(Source::Topology(name.into()))
    }
}

impl<V> std::ops::Deref for Reference<V> {
    type Target = Source;

    fn deref(&self) -> &Source {
        &self.source
    }
}

impl Source {
    /// What it points at, for the user to read: `frame1.x`,
    /// `bolt: head/top`.
    pub fn described(&self) -> String {
        match self {
            Source::Datum(name) | Source::Topology(name) | Source::Profile(name) => name.clone(),
            Source::FrameAxis { frame, axis } => format!("{frame}.{}", axis.name()),
            Source::Placed { instance, source } => format!("{instance}: {}", source.described()),
        }
    }
}

/// What `reference` points at, for the user to read; that nothing is
/// picked, if nothing is.
pub fn described<V>(reference: &Option<Reference<V>>) -> String {
    reference
        .as_ref()
        .map_or_else(|| "(not picked)".to_owned(), |r| r.source.described())
}

impl<V> std::borrow::Borrow<Source> for Reference<V> {
    fn borrow(&self) -> &Source {
        &self.source
    }
}

impl<V> Reference<V> {
    /// The sources of `references`, in order.
    pub fn sources(references: &[Self]) -> Vec<Source> {
        references.iter().map(|r| r.source.clone()).collect()
    }

    /// References to `sources`, in order.
    pub fn each(sources: impl IntoIterator<Item = Source>) -> Vec<Self> {
        sources.into_iter().map(Self::new).collect()
    }
}

impl<V> From<Source> for Reference<V> {
    fn from(source: Source) -> Self {
        Self::new(source)
    }
}

/// What a reference field takes: the kinds of thing it may point at.
/// A type references [resolve](Resolve) to takes its one kind; the
/// markers below are for fields that take several, or a kind nothing
/// resolves to.
pub trait Refers {
    /// The kinds, the one a picker starts with first.
    const KINDS: &'static [ReferenceKind];
}

impl<V: Resolve> Refers for V {
    const KINDS: &'static [ReferenceKind] = &[V::KIND];
}

/// A sketch: what a reference to a profile points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sketched;

impl Refers for Sketched {
    const KINDS: &'static [ReferenceKind] = &[ReferenceKind::Profile];
}

/// A face, or a whole body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceOrBody;

impl Refers for FaceOrBody {
    const KINDS: &'static [ReferenceKind] = &[ReferenceKind::Face, ReferenceKind::Body];
}

/// A sketch, or a body (a curve in space, to sweep along).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SketchOrBody;

impl Refers for SketchOrBody {
    const KINDS: &'static [ReferenceKind] = &[ReferenceKind::Profile, ReferenceKind::Body];
}

/// A plane or a line: a datum plane or a flat face, or a datum axis
/// or a straight edge — what a distance is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaneOrAxis;

impl Refers for PlaneOrAxis {
    const KINDS: &'static [ReferenceKind] = &[ReferenceKind::Plane, ReferenceKind::Axis];
}

/// Edges: an edge, or a face or a body for all of its edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgesOf;

impl Refers for EdgesOf {
    const KINDS: &'static [ReferenceKind] = &[
        ReferenceKind::Edge,
        ReferenceKind::Face,
        ReferenceKind::Body,
    ];
}

/// Anything that can be picked: for fields whose kinds depend on how
/// the operation is set (a construction's references).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anything;

impl Refers for Anything {
    const KINDS: &'static [ReferenceKind] = &[
        ReferenceKind::Vertex,
        ReferenceKind::Point,
        ReferenceKind::Edge,
        ReferenceKind::Face,
        ReferenceKind::Axis,
        ReferenceKind::Plane,
        ReferenceKind::Frame,
        ReferenceKind::Profile,
        ReferenceKind::Body,
    ];
}

// Manual impls: derives would needlessly require `V` to implement them.
impl<V> Clone for Reference<V> {
    fn clone(&self) -> Self {
        Self::new(self.source.clone())
    }
}

impl<V> fmt::Debug for Reference<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Reference").field(&self.source).finish()
    }
}

impl<V> PartialEq for Reference<V> {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl<V> PartialEq<Source> for Reference<V> {
    fn eq(&self, other: &Source) -> bool {
        self.source == *other
    }
}

impl<V> Eq for Reference<V> {}

impl<V> Hash for Reference<V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.source.hash(state)
    }
}

/// A reference is stored as its source alone.
impl<V> serde::Serialize for Reference<V> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.source.serialize(serializer)
    }
}

impl<'de, V> serde::Deserialize<'de> for Reference<V> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Source::deserialize(deserializer).map(Reference::new)
    }
}

/// Why a reference did not resolve.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    /// No datum has this name.
    #[error("no datum named {0:?}")]
    UnknownDatum(String),
    /// No topological entity has this name.
    #[error("no topological entity named {0:?}")]
    UnknownName(String),
    /// The name exists but is a different kind of thing.
    #[error("{name:?} is not a {expected}")]
    WrongKind {
        /// The name.
        name: String,
        /// What the reference needed.
        expected: &'static str,
    },
    /// This kind of source cannot give the type asked for (e.g. a frame
    /// from a topological name).
    #[error("{reference:?} cannot be a {expected}")]
    WrongSource {
        /// The source.
        reference: Source,
        /// The type asked for.
        expected: &'static str,
    },
    /// The edge is not a straight line (within its tolerance).
    #[error("edge {0:?} is not linear")]
    NotLinear(String),
    /// The face is not planar (within its tolerance).
    #[error("face {0:?} is not planar")]
    NotPlanar(String),
}

/// What a reference resolves to: what a reference field takes, and what
/// a picker filters for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReferenceKind {
    /// A named topological vertex.
    Vertex,
    /// A position: a frame's origin or a vertex.
    Point,
    /// A named topological edge.
    Edge,
    /// A named topological face.
    Face,
    /// A line: a datum axis, a frame's axis, or a linear edge.
    Axis,
    /// A plane: a datum plane, a frame's plane, or a planar face.
    Plane,
    /// A datum frame.
    Frame,
    /// A sketch: a profile.
    Profile,
    /// A named body.
    Body,
}

impl ReferenceKind {
    /// The kind's name, e.g. `"plane"`.
    pub fn name(self) -> &'static str {
        match self {
            ReferenceKind::Vertex => "vertex",
            ReferenceKind::Point => "point",
            ReferenceKind::Edge => "edge",
            ReferenceKind::Face => "face",
            ReferenceKind::Axis => "axis",
            ReferenceKind::Plane => "plane",
            ReferenceKind::Frame => "frame",
            ReferenceKind::Profile => "sketch",
            ReferenceKind::Body => "body",
        }
    }

    /// Whether `source` resolves to this kind in `world`.
    pub fn accepts(self, world: &World, source: &Source) -> bool {
        match self {
            ReferenceKind::Vertex => VertexId::resolve(world, source).is_ok(),
            ReferenceKind::Point => Position::resolve(world, source).is_ok(),
            ReferenceKind::Edge => EdgeId::resolve(world, source).is_ok(),
            ReferenceKind::Face => FaceId::resolve(world, source).is_ok(),
            ReferenceKind::Axis => DatumAxis::resolve(world, source).is_ok(),
            ReferenceKind::Plane => DatumPlane::resolve(world, source).is_ok(),
            ReferenceKind::Frame => DatumFrame::resolve(world, source).is_ok(),
            ReferenceKind::Profile => {
                matches!(source, Source::Profile(name) if world.profiles.contains_key(name))
            }
            ReferenceKind::Body => BodyId::resolve(world, source).is_ok(),
        }
    }
}

/// A type a [`Reference`] can resolve to.
pub trait Resolve: Sized {
    /// The kind of the type, for error messages and pickers.
    const KIND: ReferenceKind;

    /// Resolves `source` in `world` as `Self`.
    fn resolve(world: &World, source: &Source) -> Result<Self, ResolveError>;
}

impl World {
    /// Resolves `reference` against the current world.
    pub fn resolve<V: Resolve>(&self, reference: &Reference<V>) -> Result<V, ResolveError> {
        V::resolve(self, &reference.source)
    }

    /// What `source` of the instance `instance` gives, moved to where
    /// the instance is by `moved`.
    fn in_placed<V: Resolve>(
        &self,
        instance: &str,
        source: &Source,
        moved: impl Fn(V, &crate::Pose) -> Option<V>,
    ) -> Result<V, ResolveError> {
        let placed = self
            .instances
            .get(instance)
            .ok_or_else(|| ResolveError::UnknownName(instance.to_owned()))?;
        let value = V::resolve(&placed.world, source)?;
        moved(value, &placed.pose).ok_or_else(|| wrong(instance, V::KIND.name()))
    }

    fn datum(&self, name: &str) -> Result<&Datum, ResolveError> {
        self.datums
            .get(name)
            .ok_or_else(|| ResolveError::UnknownDatum(name.to_owned()))
    }

    fn frame(&self, name: &str) -> Result<&DatumFrame, ResolveError> {
        match self.datum(name)? {
            Datum::Frame(f) => Ok(f),
            _ => Err(wrong(name, DatumFrame::KIND.name())),
        }
    }

    fn topo_id<T: TryFrom<EntityId>>(
        &self,
        name: &str,
        expected: &'static str,
    ) -> Result<T, ResolveError> {
        let id = self
            .topo
            .referred(name)
            .ok_or_else(|| ResolveError::UnknownName(name.to_owned()))?;
        T::try_from(id).map_err(|_| wrong(name, expected))
    }

    /// The line through a linear edge, oriented start → end. Linear means
    /// every control point lies within the edge's tolerance of that line.
    fn edge_axis(&self, name: &str) -> Result<DatumAxis, ResolveError> {
        let id: EdgeId = self.topo_id(name, "edge")?;
        let not_linear = || ResolveError::NotLinear(name.to_owned());
        let edge = self.topo.edge(id).ok_or_else(not_linear)?;
        let curve = self.topo.curve(edge.curve).ok_or_else(not_linear)?;
        let origin = curve.point_at(edge.t0);
        let axis =
            DatumAxis::new(origin, curve.point_at(edge.t1) - origin).map_err(|_| not_linear())?;
        let straight = curve.control_points().iter().all(|&p| {
            (p - origin)
                .reject_from_normalized(axis.direction())
                .length()
                <= edge.tolerance
        });
        if straight {
            Ok(axis)
        } else {
            Err(not_linear())
        }
    }

    /// The axis of a round face — the wall of a cylinder or of a cone —
    /// through where the lines along its normals meet: its origin where
    /// the first point looked at falls on it.
    fn face_axis(&self, name: &str) -> Result<DatumAxis, ResolveError> {
        let id: FaceId = self.topo_id(name, "face")?;
        let not_round = || ResolveError::NotLinear(name.to_owned());
        let face = self.topo.face(id).ok_or_else(not_round)?;
        let surface = self.topo.surface(face.surface).ok_or_else(not_round)?;
        let ((u0, u1), (v0, v1)) = surface.domain();
        // Points of it, and its normal at each.
        let mut samples = Vec::new();
        for i in 0..7 {
            for j in 0..7 {
                let at = |lo: f64, hi: f64, k: usize| lo + (hi - lo) * (k as f64 + 0.5) / 7.0;
                let (p, du, dv) = surface.derivatives_at(at(u0, u1, i), at(v0, v1, j));
                if let Some(n) = du.cross(dv).try_normalize() {
                    samples.push((p, n));
                }
            }
        }
        let (first, n0) = *samples.first().ok_or_else(not_round)?;
        // Around an axis every normal leans the same way along it: the
        // axis is across all that the normals differ by.
        let differ: Vec<Point3> = samples.iter().map(|s| s.1 - n0).collect();
        let mut along = Point3::ZERO;
        for a in &differ {
            for b in &differ {
                let across = a.cross(*b);
                if across.length_squared() > along.length_squared() {
                    along = across;
                }
            }
        }
        if along.length() < 1e-3 {
            return Err(not_round());
        }
        let along = along.normalize();
        if differ.iter().any(|d| d.dot(along).abs() > 1e-6) {
            return Err(not_round());
        }
        // Seen along it, the normals' lines meet at one point.
        let (e1, e2) = along.any_orthonormal_pair();
        let flat = |p: Point3| crate::math::Point2::new(p.dot(e1), p.dot(e2));
        let lines: Vec<_> = samples.iter().map(|s| (flat(s.0), flat(s.1))).collect();
        let (q0, m0) = lines[0];
        let other = lines
            .iter()
            .max_by(|a, b| m0.perp_dot(a.1).abs().total_cmp(&m0.perp_dot(b.1).abs()))
            .ok_or_else(not_round)?;
        let turn = m0.perp_dot(other.1);
        if turn.abs() < 1e-6 {
            return Err(not_round());
        }
        let centre = q0 + m0 * ((other.0 - q0).perp_dot(other.1) / turn);
        let size = (q0 - centre).length().max(face.tolerance);
        let meet = lines.iter().all(|(q, m)| {
            m.length() < 1e-9 || (centre - *q).perp_dot(m.normalize()).abs() <= size * 1e-6
        });
        if !meet {
            return Err(not_round());
        }
        let on_axis = e1 * centre.x + e2 * centre.y + along * first.dot(along);
        DatumAxis::new(on_axis, along).map_err(|_| not_round())
    }

    /// The plane of a planar face, laid out by where the plane is and
    /// nothing else (see [`DatumPlane::laid_out`]): its normal out of the
    /// body, its origin where the world's origin falls on it. (Not by the face's own
    /// surface: a sketch on the face then stays where it is when the
    /// face grows, or is made one with another in its plane.) Planar
    /// means every control point lies within the face's tolerance of
    /// that plane.
    fn face_plane(&self, name: &str) -> Result<DatumPlane, ResolveError> {
        let id: FaceId = self.topo_id(name, "face")?;
        let not_planar = || ResolveError::NotPlanar(name.to_owned());
        let face = self.topo.face(id).ok_or_else(not_planar)?;
        let surface = self.topo.surface(face.surface).ok_or_else(not_planar)?;
        let (nu, nv) = surface.counts();
        // Sum of the control-net cell normals, the direction of ∂u × ∂v:
        // each the cross product of the cell's diagonals, which survives a
        // collapsed side (as at a disk's centre).
        let mut normal = crate::math::Point3::ZERO;
        for i in 0..nu - 1 {
            for j in 0..nv - 1 {
                let p = |di, dj| surface.control_point(i + di, j + dj);
                normal += (p(1, 1) - p(0, 0)).cross(p(0, 1) - p(1, 0));
            }
        }
        let normal = normal.try_normalize().ok_or_else(not_planar)?;
        let corner = surface.control_point(0, 0);
        if surface
            .control_points()
            .iter()
            .any(|&p| (p - corner).dot(normal).abs() > face.tolerance)
        {
            return Err(not_planar());
        }
        let normal = if self.topo.faces_outwards(id) == Some(false) {
            -normal
        } else {
            normal
        };
        let origin = normal * corner.dot(normal);
        DatumPlane::laid_out(origin, normal).map_err(|_| not_planar())
    }
}

fn wrong(name: &str, expected: &'static str) -> ResolveError {
    ResolveError::WrongKind {
        name: name.to_owned(),
        expected,
    }
}

fn wrong_source<V: Resolve>(source: &Source) -> ResolveError {
    ResolveError::WrongSource {
        reference: source.clone(),
        expected: V::KIND.name(),
    }
}

macro_rules! resolve_topology {
    ($($ty:ty => $kind:ident;)*) => {$(
        impl Resolve for $ty {
            const KIND: ReferenceKind = ReferenceKind::$kind;

            fn resolve(world: &World, source: &Source) -> Result<Self, ResolveError> {
                match source {
                    Source::Topology(name) => world.topo_id(name, Self::KIND.name()),
                    _ => Err(wrong_source::<Self>(source)),
                }
            }
        }
    )*};
}

resolve_topology! {
    VertexId => Vertex;
    EdgeId => Edge;
    FaceId => Face;
    BodyId => Body;
}

/// Where a point reference is: a frame's origin or a vertex. Not a datum
/// of its own; what constructions take in their point slots.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position(pub Point3);

impl Resolve for Position {
    const KIND: ReferenceKind = ReferenceKind::Point;

    fn resolve(world: &World, source: &Source) -> Result<Self, ResolveError> {
        match source {
            Source::Datum(name) => Ok(Position(world.frame(name)?.origin())),
            Source::Topology(name) => {
                let id: VertexId = world.topo_id(name, Self::KIND.name())?;
                let vertex = world
                    .topo
                    .vertex(id)
                    .ok_or_else(|| ResolveError::UnknownName(name.clone()))?;
                Ok(Position(vertex.point))
            }
            Source::Placed { instance, source } => {
                world.in_placed(instance, source, |p: Position, pose| {
                    Some(Position(pose.transform_point(p.0)))
                })
            }
            Source::FrameAxis { .. } | Source::Profile(_) => Err(wrong_source::<Self>(source)),
        }
    }
}

impl Resolve for DatumAxis {
    const KIND: ReferenceKind = ReferenceKind::Axis;

    fn resolve(world: &World, source: &Source) -> Result<Self, ResolveError> {
        match source {
            Source::Datum(name) => match world.datum(name)? {
                Datum::Axis(a) => Ok(*a),
                _ => Err(wrong(name, "datum axis")),
            },
            Source::FrameAxis { frame, axis } => Ok(world.frame(frame)?.axis(*axis)),
            // A straight edge, or a round face's axis.
            Source::Topology(name) => world
                .edge_axis(name)
                .or_else(|edge| world.face_axis(name).map_err(|_| edge)),
            Source::Placed { instance, source } => {
                world.in_placed(instance, source, |a: DatumAxis, pose| {
                    DatumAxis::new(
                        pose.transform_point(a.origin()),
                        pose.transform_vector(a.direction()),
                    )
                    .ok()
                })
            }
            Source::Profile(_) => Err(wrong_source::<Self>(source)),
        }
    }
}

impl Resolve for DatumPlane {
    const KIND: ReferenceKind = ReferenceKind::Plane;

    fn resolve(world: &World, source: &Source) -> Result<Self, ResolveError> {
        match source {
            Source::Datum(name) => match world.datum(name)? {
                Datum::Plane(p) => Ok(*p),
                _ => Err(wrong(name, "datum plane")),
            },
            Source::FrameAxis { frame, axis } => Ok(world.frame(frame)?.plane(*axis)),
            Source::Topology(name) => world.face_plane(name),
            Source::Placed { instance, source } => {
                world.in_placed(instance, source, |p: DatumPlane, pose| {
                    DatumPlane::new(
                        pose.transform_point(p.origin()),
                        pose.transform_vector(p.normal()),
                        pose.transform_vector(p.x_dir()),
                    )
                    .ok()
                })
            }
            Source::Profile(_) => Err(wrong_source::<Self>(source)),
        }
    }
}

impl Resolve for DatumFrame {
    const KIND: ReferenceKind = ReferenceKind::Frame;

    fn resolve(world: &World, source: &Source) -> Result<Self, ResolveError> {
        match source {
            Source::Datum(name) => world.frame(name).copied(),
            Source::Placed { instance, source } => {
                world.in_placed(instance, source, |f: DatumFrame, pose| {
                    DatumFrame::from_pose(*pose * f.pose()).ok()
                })
            }
            _ => Err(wrong_source::<Self>(source)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Point3;
    use crate::world::ORIGIN;
    use crate::world::topo::tests::square;

    fn square_world() -> anyhow::Result<World> {
        let (topo, _, _) = square()?;
        Ok(World {
            topo,
            ..World::new()
        })
    }

    #[test]
    fn origin_frame_is_there_by_default() -> anyhow::Result<()> {
        let w = World::new();
        assert_eq!(
            w.resolve(&Reference::<DatumFrame>::datum(ORIGIN))?,
            DatumFrame::global()
        );
        Ok(())
    }

    #[test]
    fn datums_resolve_by_kind() -> anyhow::Result<()> {
        let mut w = World::new();
        w.datums.insert("top".into(), DatumPlane::xy().into());
        assert_eq!(
            w.resolve(&Reference::<DatumPlane>::datum("top"))?,
            DatumPlane::xy()
        );
        assert!(matches!(
            w.resolve(&Reference::<DatumAxis>::datum("top")),
            Err(ResolveError::WrongKind { .. })
        ));
        assert!(matches!(
            w.resolve(&Reference::<DatumFrame>::datum("nope")),
            Err(ResolveError::UnknownDatum(_))
        ));
        assert!(matches!(
            w.resolve(&Reference::<DatumFrame>::topology("top")),
            Err(ResolveError::WrongSource { .. })
        ));
        Ok(())
    }

    #[test]
    fn frames_give_axes_and_planes() -> anyhow::Result<()> {
        let w = World::new();
        let axis = w.resolve(&Reference::<DatumAxis>::frame_axis(ORIGIN, Axis::Y))?;
        assert!(axis.direction().abs_diff_eq(Point3::Y, 1e-12));
        let plane = w.resolve(&Reference::<DatumPlane>::frame_axis(ORIGIN, Axis::X))?;
        assert!(plane.normal().abs_diff_eq(Point3::X, 1e-12));
        assert!(plane.x_dir().abs_diff_eq(Point3::Y, 1e-12));
        Ok(())
    }

    #[test]
    fn linear_edges_are_axes_and_planar_faces_are_planes() -> anyhow::Result<()> {
        let w = square_world()?;
        let axis = w.resolve(&Reference::<DatumAxis>::topology("square/edge/0"))?;
        assert!(axis.direction().abs_diff_eq(Point3::X, 1e-12));
        let plane = w.resolve(&Reference::<DatumPlane>::topology("square/face"))?;
        assert!(plane.normal().abs_diff_eq(Point3::Z, 1e-12));
        assert!(matches!(
            w.resolve(&Reference::<DatumPlane>::topology("square/edge/0")),
            Err(ResolveError::WrongKind { .. })
        ));
        let edge = w.resolve(&Reference::<EdgeId>::topology("square/edge/0"))?;
        assert_eq!(w.topo.name_of(edge), Some("square/edge/0"));
        Ok(())
    }
}
