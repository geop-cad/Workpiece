//! What a step does to a world, kept so that the step is not applied
//! again where it would do the same.
//!
//! A step's result depends on its arguments and on what it
//! reads of the world while it is applied, which the builder notes
//! ([`workpiece_core::Trace`]): [`input`] hashes that. What it then did is the [`Effect`]: the datums, sketches,
//! placed parts and bodies it made, changed and removed, found by
//! comparing the world before and after ([`Index`]). Where the same
//! step meets the same input again — after an edit to something it does
//! not read, in another order of steps, in a variant that changes
//! nothing of it — its effect is applied instead of the step.
//!
//! Bodies are compared and kept whole. An effect is only kept if it can
//! be applied to the same end: the bodies it makes are solids (they are
//! kept as [`Solid`]s) and come after every body it leaves alone, as
//! they do when they are written again.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use workpiece_boolean::Solid;
use workpiece_core::world::topo::{
    BodyId, BodyKind, CoedgeKind, EdgeId, EntityId, FaceId, Topo, VertexId,
};
use workpiece_core::world::{Coupling, Datum, Instance, Joint, Profile};
use workpiece_core::{Read, Source, World};

type Hashed = std::collections::hash_map::DefaultHasher;

fn floats(h: &mut Hashed, xs: &[f64]) {
    xs.len().hash(h);
    for x in xs {
        x.to_bits().hash(h);
    }
}

fn text(value: &impl std::fmt::Debug) -> u64 {
    let mut h = Hashed::new();
    format!("{value:?}").hash(&mut h);
    h.finish()
}

fn vertex(h: &mut Hashed, topo: &Topo, id: VertexId) {
    if let Some(v) = topo.vertex(id) {
        topo.name_of(id).hash(h);
        floats(h, &v.point.to_array());
        v.tolerance.to_bits().hash(h);
    }
}

fn edge(h: &mut Hashed, topo: &Topo, id: EdgeId) {
    let Some(e) = topo.edge(id) else {
        return;
    };
    topo.name_of(id).hash(h);
    floats(h, &[e.t0, e.t1, e.tolerance]);
    if let Some(curve) = topo.curve(e.curve) {
        curve.degree().hash(h);
        floats(h, curve.knots());
        floats(h, curve.weights());
        for p in curve.control_points() {
            floats(h, &p.to_array());
        }
    }
    vertex(h, topo, e.start);
    vertex(h, topo, e.end);
}

fn face(h: &mut Hashed, topo: &Topo, id: FaceId) {
    let Some(f) = topo.face(id) else {
        return;
    };
    topo.name_of(id).hash(h);
    f.tolerance.to_bits().hash(h);
    if let Some(surface) = topo.surface(f.surface) {
        surface.degrees().hash(h);
        surface.counts().hash(h);
        let (ku, kv) = surface.knots();
        floats(h, ku);
        floats(h, kv);
        floats(h, surface.weights());
        for p in surface.control_points() {
            floats(h, &p.to_array());
        }
    }
    for &l in &f.loops {
        let Some(lp) = topo.loop_(l) else {
            continue;
        };
        lp.coedges.len().hash(h);
        for &c in &lp.coedges {
            let Some(coedge) = topo.coedge(c) else {
                continue;
            };
            match coedge.kind {
                CoedgeKind::Edge {
                    edge: used,
                    orientation,
                } => {
                    format!("{orientation:?}").hash(h);
                    edge(h, topo, used);
                }
                CoedgeKind::Pole { vertex: at, .. } => vertex(h, topo, at),
            }
            if let Some(pcurve) = coedge.pcurve.and_then(|p| topo.pcurve(p)) {
                pcurve.degree().hash(h);
                floats(h, pcurve.knots());
                floats(h, pcurve.weights());
                for p in pcurve.control_points() {
                    floats(h, &p.to_array());
                }
            }
        }
    }
}

/// What a body is, hashed: its kind, and every face of it with its
/// edges and vertices, their names and their geometry.
fn body(topo: &Topo, id: BodyId) -> u64 {
    let mut h = Hashed::new();
    topo.name_of(id).hash(&mut h);
    if let Some(b) = topo.body(id) {
        format!("{:?}", b.kind).hash(&mut h);
        (b.free_edges.len(), b.free_vertices.len()).hash(&mut h);
    }
    for f in topo.body_faces(id).unwrap_or_default() {
        face(&mut h, topo, f);
    }
    h.finish()
}

/// The bodies of `topo` that have a name, in the order they were made.
fn bodies(topo: &Topo) -> Vec<(String, BodyId)> {
    topo.bodies()
        .ids()
        .filter_map(|b| Some((topo.name_of(b)?.to_owned(), b)))
        .collect()
}

/// What a world holds, each thing by its name with what it is, hashed:
/// to tell what a step changed.
pub(crate) struct Index {
    datums: BTreeMap<String, u64>,
    profiles: BTreeMap<String, u64>,
    instances: BTreeMap<String, u64>,
    /// In the order they were made.
    bodies: Vec<(String, u64)>,
    joints: u64,
    couplings: u64,
    annotations: BTreeMap<String, u64>,
    looks: BTreeMap<String, u64>,
    threads: BTreeMap<String, u64>,
    views: BTreeMap<String, u64>,
    sections: BTreeMap<String, u64>,
    masses: BTreeMap<String, u64>,
}

fn placed(instance: &Instance) -> u64 {
    text(&(instance.world.stamp, instance.pose, instance.fixed))
}

impl Index {
    pub(crate) fn of(world: &World) -> Self {
        Index {
            datums: world
                .datums
                .iter()
                .map(|(n, d)| (n.clone(), text(d)))
                .collect(),
            profiles: world
                .profiles
                .iter()
                .map(|(n, p)| (n.clone(), text(p)))
                .collect(),
            instances: world
                .instances
                .iter()
                .map(|(n, i)| (n.clone(), placed(i)))
                .collect(),
            bodies: bodies(&world.topo)
                .into_iter()
                .map(|(name, id)| (name, body(&world.topo, id)))
                .collect(),
            joints: text(&world.joints),
            couplings: text(&world.couplings),
            annotations: world
                .annotations
                .iter()
                .map(|(n, a)| (n.clone(), text(a)))
                .collect(),
            looks: world
                .looks
                .iter()
                .map(|(n, l)| (n.clone(), text(l)))
                .collect(),
            threads: world
                .threads
                .iter()
                .map(|(n, t)| (n.clone(), text(t)))
                .collect(),
            views: world
                .views
                .iter()
                .map(|(n, v)| (n.clone(), text(v)))
                .collect(),
            sections: world
                .sections
                .iter()
                .map(|(n, c)| (n.clone(), text(c)))
                .collect(),
            masses: world
                .masses
                .iter()
                .map(|(n, m)| (n.clone(), text(m)))
                .collect(),
        }
    }

    /// The same without the bodies: for telling what else a step
    /// changed, where the bodies it wrote are known.
    pub(crate) fn light(world: &World) -> Self {
        let mut all = World::new();
        all.datums.clone_from(&world.datums);
        all.profiles.clone_from(&world.profiles);
        all.instances.clone_from(&world.instances);
        all.joints.clone_from(&world.joints);
        all.couplings.clone_from(&world.couplings);
        all.annotations.clone_from(&world.annotations);
        all.looks.clone_from(&world.looks);
        all.threads.clone_from(&world.threads);
        all.views.clone_from(&world.views);
        all.sections.clone_from(&world.sections);
        all.masses.clone_from(&world.masses);
        Self::of(&all)
    }

    fn all(&self) -> u64 {
        let mut h = Hashed::new();
        self.datums.hash(&mut h);
        self.profiles.hash(&mut h);
        self.instances.hash(&mut h);
        // (Whatever order the bodies were made in.)
        let by_name: BTreeMap<&str, u64> =
            self.bodies.iter().map(|b| (b.0.as_str(), b.1)).collect();
        by_name.hash(&mut h);
        (self.joints, self.couplings).hash(&mut h);
        self.annotations.hash(&mut h);
        self.looks.hash(&mut h);
        self.threads.hash(&mut h);
        self.views.hash(&mut h);
        self.sections.hash(&mut h);
        self.masses.hash(&mut h);
        h.finish()
    }
}

/// The body each face, edge and vertex of `topo` is of.
fn owners(topo: &Topo) -> HashMap<EntityId, BodyId> {
    let mut of = HashMap::new();
    for (_, id) in bodies(topo) {
        for f in topo.body_faces(id).unwrap_or_default() {
            of.insert(f.into(), id);
            let Some(face) = topo.face(f) else {
                continue;
            };
            let coedges = face
                .loops
                .iter()
                .filter_map(|&l| topo.loop_(l))
                .flat_map(|l| &l.coedges);
            for &c in coedges {
                match topo.coedge(c).map(|c| c.kind) {
                    Some(CoedgeKind::Edge { edge, .. }) => {
                        of.insert(edge.into(), id);
                        if let Some(e) = topo.edge(edge) {
                            of.insert(e.start.into(), id);
                            of.insert(e.end.into(), id);
                        }
                    }
                    Some(CoedgeKind::Pole { vertex, .. }) => {
                        of.insert(vertex.into(), id);
                    }
                    None => {}
                }
            }
        }
    }
    of
}

/// What `source` names in `world`, hashed; with `whole`, the body a
/// piece of topology is of instead of the piece.
fn named(
    h: &mut Hashed,
    world: &World,
    source: &Source,
    whole: Option<&HashMap<EntityId, BodyId>>,
) {
    format!("{source:?}").hash(h);
    match source {
        Source::Datum(name) => world.datums.get(name).map(text).hash(h),
        Source::FrameAxis { frame, .. } => world.datums.get(frame).map(text).hash(h),
        Source::Profile(name) => world.profiles.get(name).map(text).hash(h),
        Source::Topology(name) => {
            let topo = &world.topo;
            let Some(id) = topo.referred(name) else {
                "missing".hash(h);
                return;
            };
            if let Some(owner) = whole.and_then(|of| of.get(&id)) {
                body(topo, *owner).hash(h);
                return;
            }
            match id {
                EntityId::Body(b) => body(topo, b).hash(h),
                EntityId::Face(f) => face(h, topo, f),
                EntityId::Edge(e) => edge(h, topo, e),
                EntityId::Vertex(v) => vertex(h, topo, v),
                other => format!("{other:?}").hash(h),
            }
        }
        // Something of a placed part: the part as it was built, and
        // where it is.
        Source::Placed { instance, .. } => world.instances.get(instance).map(placed).hash(h),
    }
}

/// What a step that read `reads` of a world depends on of `world`,
/// hashed: the same, where it would read the same again.
pub(crate) fn input(world: &World, reads: &[Read]) -> u64 {
    let mut h = Hashed::new();
    if reads.contains(&Read::Everything) {
        Index::of(world).all().hash(&mut h);
        return h.finish();
    }
    let of = reads
        .iter()
        .any(|r| matches!(r, Read::Owner(_)))
        .then(|| owners(&world.topo));
    for read in reads {
        match read {
            Read::Source(source) => named(&mut h, world, source, None),
            Read::Owner(source) => named(&mut h, world, source, of.as_ref()),
            Read::Body(name) => {
                name.hash(&mut h);
                let id = world.topo.names().id_of(name);
                let id = id.and_then(|id| BodyId::try_from(id).ok());
                id.map(|b| body(&world.topo, b)).hash(&mut h);
            }
            Read::Listing => {
                for (name, id) in bodies(&world.topo) {
                    name.hash(&mut h);
                    format!("{:?}", world.topo.body(id).map(|b| b.kind)).hash(&mut h);
                }
            }
            Read::Instance(name) => {
                name.hash(&mut h);
                world.instances.get(name).map(placed).hash(&mut h);
            }
            Read::Placed => {
                let names: Vec<&String> = world.instances.keys().collect();
                names.hash(&mut h);
            }
            Read::Reach(name) => {
                name.hash(&mut h);
                let reach = world.instances.get(name).and_then(Instance::reach);
                reach.map(|(lo, hi)| (text(&lo), text(&hi))).hash(&mut h);
            }
            Read::Joint(name) => {
                name.hash(&mut h);
                world
                    .joints
                    .iter()
                    .find(|j| j.name == *name)
                    .map(text)
                    .hash(&mut h);
            }
            Read::Bodies => {
                let all: BTreeMap<String, u64> = bodies(&world.topo)
                    .into_iter()
                    .map(|(name, id)| (name, body(&world.topo, id)))
                    .collect();
                all.hash(&mut h);
            }
            Read::Everything => {}
        }
    }
    h.finish()
}

/// What a step did to a world.
pub(crate) struct Effect {
    datums: Vec<(String, Option<Datum>)>,
    profiles: Vec<(String, Option<Profile>)>,
    instances: Vec<(String, Option<Instance>)>,
    /// The bodies gone (or changed, and so made again).
    gone: Vec<String>,
    /// The bodies made, in order.
    made: Vec<(String, Solid)>,
    joints: Option<Vec<Joint>>,
    couplings: Option<Vec<Coupling>>,
    annotations: Vec<(String, Option<workpiece_core::world::Annotation>)>,
    looks: Vec<(String, Option<workpiece_core::world::Look>)>,
    threads: Vec<(String, Option<workpiece_core::world::Thread>)>,
    views: Vec<(String, Option<workpiece_core::world::Viewpoint>)>,
    sections: Vec<(String, Option<workpiece_core::world::Cutting>)>,
    masses: Vec<(String, Option<workpiece_core::world::MassProperties>)>,
    /// What the step left in the part's cache.
    cache: Vec<(String, Vec<f64>)>,
}

/// What is in `after` and not (so) in `before`, with what it is now,
/// and what is only in `before`, with nothing.
fn changed<T: Clone>(
    before: &BTreeMap<String, u64>,
    after: &BTreeMap<String, u64>,
    now: &BTreeMap<String, T>,
) -> Vec<(String, Option<T>)> {
    let mut out: Vec<(String, Option<T>)> = after
        .iter()
        .filter(|(name, hash)| before.get(*name) != Some(*hash))
        .filter_map(|(name, _)| Some((name.clone(), Some(now.get(name)?.clone()))))
        .collect();
    out.extend(
        before
            .keys()
            .filter(|name| !after.contains_key(*name))
            .map(|name| (name.clone(), None)),
    );
    out
}

impl Effect {
    /// What made `world` of the world `before` was taken of: `cache` is
    /// what the step left in the part's cache. `None` if it cannot be
    /// kept to be applied again.
    ///
    /// `wrote` are the bodies the step wrote, as the world noted them
    /// (see [`Topo::take_written`]): the others are as they were in
    /// `was`, the world before, and are not looked at.
    pub(crate) fn of(
        before: &Index,
        was: &World,
        world: &World,
        wrote: &std::collections::BTreeSet<String>,
        cache: Vec<(String, Vec<f64>)>,
    ) -> Option<Self> {
        let after = Index::light(world);
        let now = bodies(&world.topo);
        // (That holds: no body changed that the step is not said to
        // have written.)
        debug_assert!(
            {
                let old: BTreeMap<String, u64> = bodies(&was.topo)
                    .into_iter()
                    .map(|(name, id)| (name, body(&was.topo, id)))
                    .collect();
                let new: BTreeMap<String, u64> = now
                    .iter()
                    .map(|(name, id)| (name.clone(), body(&world.topo, *id)))
                    .collect();
                let names = old.keys().chain(new.keys());
                names
                    .filter(|n| old.get(*n) != new.get(*n))
                    .all(|n| wrote.contains(n))
            },
            "a body changed that was not written: {wrote:?}"
        );
        let gone: Vec<String> = bodies(&was.topo)
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| wrote.contains(name))
            .collect();
        // What it made has to come after all it left alone: that is
        // where it is put when the effect is applied.
        let fresh = |name: &str| wrote.contains(name);
        let first = now.iter().position(|(n, _)| fresh(n));
        if let Some(first) = first
            && now[first..].iter().any(|(n, _)| !fresh(n))
        {
            return None;
        }
        let topo = &world.topo;
        let mut made = Vec::new();
        for (name, _) in &now[first.unwrap_or(now.len())..] {
            let id = topo.resolve::<BodyId>(name)?;
            if topo.body(id)?.kind != BodyKind::Solid {
                return None;
            }
            made.push((name.clone(), workpiece_boolean::read(topo, id).ok()?));
        }
        Some(Effect {
            datums: changed(&before.datums, &after.datums, &world.datums),
            profiles: changed(&before.profiles, &after.profiles, &world.profiles),
            instances: changed(&before.instances, &after.instances, &world.instances),
            gone,
            made,
            joints: (before.joints != after.joints).then(|| world.joints.clone()),
            couplings: (before.couplings != after.couplings).then(|| world.couplings.clone()),
            annotations: changed(&before.annotations, &after.annotations, &world.annotations),
            looks: changed(&before.looks, &after.looks, &world.looks),
            threads: changed(&before.threads, &after.threads, &world.threads),
            views: changed(&before.views, &after.views, &world.views),
            sections: changed(&before.sections, &after.sections, &world.sections),
            masses: changed(&before.masses, &after.masses, &world.masses),
            cache,
        })
    }

    /// Makes the bodies the step made in `world` (the world it was
    /// applied to) what applying the effect makes them: written from
    /// what is kept of them, so that a world is the same whether its
    /// steps were applied or found kept.
    pub(crate) fn settle(&self, world: &mut World) -> anyhow::Result<()> {
        for (name, _) in &self.made {
            if let Some(id) = world.topo.resolve::<BodyId>(name) {
                workpiece_boolean::remove(&mut world.topo, id);
            }
        }
        for (name, solid) in &self.made {
            workpiece_boolean::write(&mut world.topo, solid, name)?;
        }
        Ok(())
    }

    /// Does to `world` what the step did; what it left in the cache is
    /// returned, to be kept as the step's.
    pub(crate) fn apply(&self, world: &mut World) -> anyhow::Result<&[(String, Vec<f64>)]> {
        fn put<T: Clone>(into: &mut BTreeMap<String, T>, from: &[(String, Option<T>)]) {
            for (name, value) in from {
                match value {
                    Some(value) => {
                        into.insert(name.clone(), value.clone());
                    }
                    None => {
                        into.remove(name);
                    }
                }
            }
        }
        put(&mut world.datums, &self.datums);
        put(&mut world.profiles, &self.profiles);
        put(&mut world.instances, &self.instances);
        for name in &self.gone {
            if let Some(id) = world.topo.resolve::<BodyId>(name) {
                workpiece_boolean::remove(&mut world.topo, id);
            }
        }
        for (name, solid) in &self.made {
            workpiece_boolean::write(&mut world.topo, solid, name)?;
        }
        if let Some(joints) = &self.joints {
            world.joints.clone_from(joints);
        }
        if let Some(couplings) = &self.couplings {
            world.couplings.clone_from(couplings);
        }
        put(&mut world.annotations, &self.annotations);
        put(&mut world.looks, &self.looks);
        put(&mut world.threads, &self.threads);
        put(&mut world.views, &self.views);
        put(&mut world.sections, &self.sections);
        put(&mut world.masses, &self.masses);
        Ok(&self.cache)
    }
}
