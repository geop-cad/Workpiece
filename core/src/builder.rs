//! What operations are applied to: the world being built, the part's
//! [`Cache`] of what earlier builds worked out, and a way to build other
//! parts into it.
//!
//! The [`Builder`] trait is implemented where parts live (the workspace);
//! [`Standalone`] applies operations to a lone world, without parts.
//!
//! Every builder keeps one invariant: **a step never depends on a later
//! step.** Within a part, a step sees only what the steps before it made;
//! across parts, a part never (directly or through others) contains
//! itself, so [`Builder::build`] fails for a part already being built.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use crate::measure::{Measure, Parameters};
use crate::reference::{Reference, Resolve, ResolveError, Source};
use crate::world::World;

/// Where the operations of a part start solving from: what they worked
/// out the last time the part was built, as numbers by name.
///
/// An operation that solves something (a sketch its points, later an
/// assembly the poses of its parts) starts from what it finds here and
/// leaves its solution here, under names starting with its id
/// (`"sketch1/p3"`). The arguments stay what the user asked for.
///
/// The cache is an *input* of the build, like the arguments: constraints
/// often have several solutions, and which one a build finds depends on
/// where it starts. So the cache belongs to the part. It is saved with
/// it, and once it holds a solution that solution stays the starting
/// point: a build from another cache could snap to another solution and
/// break what refers to the first. It is empty only where nothing was
/// solved yet (a line just drawn); then operations start from their
/// arguments.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Cache {
    values: BTreeMap<String, Vec<f64>>,
}

impl Cache {
    /// The numbers called `name`.
    pub fn get(&self, name: &str) -> Option<&[f64]> {
        self.values.get(name).map(Vec::as_slice)
    }

    /// Whether nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Everything under `owner` (an operation's id): the entries called
    /// `"{owner}/…"`, by the rest of their name.
    pub fn of<'a>(&'a self, owner: &'a str) -> impl Iterator<Item = (&'a str, &'a [f64])> {
        self.values.iter().filter_map(move |(name, values)| {
            let rest = name.strip_prefix(owner)?.strip_prefix('/')?;
            Some((rest, values.as_slice()))
        })
    }

    /// Moves everything under `from` to under `to`: the operation was
    /// renamed, and starts from the same solution.
    pub fn rename(&mut self, from: &str, to: &str) {
        let entries: Vec<(String, Vec<f64>)> = self
            .of(from)
            .map(|(name, values)| (name.to_owned(), values.to_vec()))
            .collect();
        self.replace(from, []);
        self.replace(to, entries);
    }

    /// Replaces everything under `owner` with `entries`, each named
    /// `"{owner}/{name}"`.
    pub fn replace(&mut self, owner: &str, entries: impl IntoIterator<Item = (String, Vec<f64>)>) {
        self.values.retain(|name, _| {
            name.strip_prefix(owner)
                .is_none_or(|rest| !rest.starts_with('/'))
        });
        for (name, values) in entries {
            self.values.insert(format!("{owner}/{name}"), values);
        }
    }
}

/// Something a step read of the world.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Read {
    /// What a source names, and no more of it: a plane, an edge, a
    /// face, a sketch, a body.
    Source(Source),
    /// The whole body that what a source names is of (an edge's body,
    /// to round the edge).
    Owner(Source),
    /// The whole body of this name.
    Body(String),
    /// Which bodies there are: their names, and what kind each is.
    Listing,
    /// Every body there is.
    Bodies,
    /// The part placed under this name, and where it is.
    Instance(String),
    /// Which parts are placed: their names.
    Placed,
    /// How far the part placed under this name reaches: the box around
    /// it, where it is.
    Reach(String),
    /// The joint of this name between parts placed.
    Joint(String),
    /// Anything.
    Everything,
}

/// What a step read of the world while it was applied, and the bodies
/// it said it wrote: kept by a builder that does not apply a step again
/// where it would read the same.
#[derive(Debug, Default)]
pub struct Trace {
    reads: Mutex<BTreeSet<Read>>,
    wrote: Mutex<BTreeSet<String>>,
}

fn held<T>(lock: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    lock.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Trace {
    /// Notes that `read` was read.
    pub fn note(&self, read: Read) {
        held(&self.reads).insert(read);
    }

    /// Notes that the body `name` was written (made, changed or
    /// removed).
    pub fn wrote(&self, name: &str) {
        held(&self.wrote).insert(name.to_owned());
    }

    /// What was noted since the last time, reads and bodies written,
    /// each in order; nothing is noted afterwards.
    pub fn take(&self) -> (Vec<Read>, Vec<String>) {
        let reads = std::mem::take(&mut *held(&self.reads));
        let wrote = std::mem::take(&mut *held(&self.wrote));
        (reads.into_iter().collect(), wrote.into_iter().collect())
    }
}

/// The world being built, and access to the other parts.
pub trait Builder {
    /// The world being built: all of it. A builder that keeps track of
    /// what a step reads takes this as reading [everything](Read::Everything);
    /// a step that reads less asks for less (see `read`, `resolve`).
    fn world(&self) -> &World;

    /// The world being built, for operations to change: all of it, read
    /// as [`world`](Self::world) is (see `change`).
    fn world_mut(&mut self) -> &mut World;

    /// The world, without that counting as reading any of it: what
    /// `read` and `resolve` look at, having noted what of it.
    fn peek(&self) -> &World {
        self.world()
    }

    /// The same, to change.
    fn peek_mut(&mut self) -> &mut World {
        self.world_mut()
    }

    /// Where what is read is noted, by a builder that keeps track.
    fn trace(&self) -> Option<&Trace> {
        None
    }

    /// What earlier builds of the part being built worked out.
    fn cache(&self) -> &Cache;

    /// The cache, for operations to leave what they worked out.
    fn cache_mut(&mut self) -> &mut Cache;

    /// The parameters of the part being built, as the variant being
    /// built sets them: what the formulas of its operations' arguments
    /// refer to.
    fn parameters(&self) -> &Parameters;

    /// The world of the part being built as it was before the first of
    /// its steps `steps`, and as it was after the last of them: what
    /// those steps did is what differs. A builder that keeps track of
    /// what a step reads takes this as reading
    /// [everything](Read::Everything). Fails where no part's steps are
    /// being built, or the part has no such steps.
    fn spanning(&self, steps: &[String]) -> anyhow::Result<(Arc<World>, Arc<World>)> {
        anyhow::bail!("the steps {steps:?} are not at hand: no part's steps are being built")
    }

    /// The steps `steps` of the part being built (and those between
    /// them), done again on `on` in place of the world they were done
    /// on: what a feature does to a body that is somewhere else — the
    /// same sketch, the same tool, another body under it. Fails where
    /// no part's steps are being built, the part has no such steps, or
    /// one of them fails there.
    fn redone(&self, steps: &[String], on: World) -> anyhow::Result<World> {
        let _ = on;
        anyhow::bail!("the steps {steps:?} are not at hand: no part's steps are being built")
    }

    /// Builds the part at `path` (its `variant`, or the part as it is by
    /// default) in a world of its own and returns it; nothing of
    /// [`world`](Self::world) changes. Fails if there is no such part or
    /// variant, if it is one being built (a cycle), or if building it
    /// fails.
    fn build(&self, path: &str, variant: Option<&str>) -> anyhow::Result<Arc<World>>;
}

impl dyn Builder + '_ {
    /// The world, to read of it: what is read of its bodies is noted
    /// as it is read (see [`Topo::watch`](crate::world::topo::Topo::watch)),
    /// and of the rest, what the operation refers to. With the same of
    /// it the step does the same and is not applied again.
    pub fn tracked(&self) -> &World {
        self.peek()
    }

    /// The world to change, read as [`tracked`](Self::tracked) reads it.
    pub fn tracked_mut(&mut self) -> &mut World {
        self.peek_mut()
    }

    fn noted(&self, read: Read) -> &World {
        if let Some(trace) = self.trace() {
            trace.note(read);
        }
        self.peek()
    }

    /// The part placed as `name`, if one is: noted as read.
    pub fn placed(&self, name: &str) -> Option<&crate::world::Instance> {
        self.noted(Read::Instance(name.to_owned()))
            .instances
            .get(name)
    }

    /// The world, to read of it the parts placed as `names` (what is on
    /// them, where they are): those are noted as read.
    pub fn with_placed<'n>(&self, names: impl IntoIterator<Item = &'n str>) -> &World {
        for name in names {
            self.noted(Read::Instance(name.to_owned()));
        }
        self.peek()
    }

    /// The names of the parts placed: noted as read.
    pub fn placed_names(&self) -> Vec<String> {
        self.noted(Read::Placed).instances.keys().cloned().collect()
    }

    /// The box around the part placed as `name` (see
    /// [`Instance::reach`](crate::world::Instance::reach)): noted as
    /// read. A step that works on what a part could touch reads this of
    /// every part, and the parts themselves only where the boxes meet.
    pub fn placed_reach(&self, name: &str) -> Option<(crate::math::Point3, crate::math::Point3)> {
        let world = self.noted(Read::Reach(name.to_owned()));
        world.instances.get(name)?.reach()
    }

    /// The joint called `name`, if there is one: noted as read.
    pub fn joint(&self, name: &str) -> Option<&crate::world::Joint> {
        let world = self.noted(Read::Joint(name.to_owned()));
        world.joints.iter().find(|j| j.name == name)
    }

    /// Notes what was looked at of a copy of the world's bodies, as
    /// read of the world.
    pub fn looked(&self, at: Vec<crate::world::topo::Looked>) {
        use crate::world::topo::Looked;
        let Some(trace) = self.trace() else {
            return;
        };
        for looked in at {
            trace.note(match looked {
                Looked::Body(name) => Read::Body(name),
                Looked::Named(name) => Read::Owner(Source::Topology(name)),
                Looked::Listing => Read::Listing,
                Looked::All => Read::Bodies,
            });
        }
    }

    /// What `reference` resolves to in the world, noted as read.
    pub fn resolve<V: Resolve>(&self, reference: &Reference<V>) -> Result<V, ResolveError> {
        if let Some(trace) = self.trace() {
            trace.note(Read::Source(reference.source.clone()));
        }
        self.peek().resolve(reference)
    }

    /// Puts what an operation made in the world. Fails if something of
    /// it is there already under its name.
    pub fn put(&mut self, made: crate::operation::Made<'_>) -> anyhow::Result<()> {
        let world = self.peek_mut();
        for (name, datum) in made.datums {
            anyhow::ensure!(!world.datums.contains_key(&name), "datum {name:?} exists");
            world.datums.insert(name, datum);
        }
        // (Writing is no reading of what is there.)
        let watched = world.topo.pause();
        for name in &made.removed {
            if let Some(body) = world.topo.resolve(name) {
                world.topo.remove_body(body);
            }
        }
        let written = made
            .bodies
            .iter()
            .try_for_each(|(name, shape)| shape.write(&mut world.topo, name));
        world.topo.resume(watched);
        written?;
        for (name, profile) in made.profiles {
            anyhow::ensure!(
                !world.profiles.contains_key(&name),
                "sketch {name:?} exists"
            );
            world.profiles.insert(name, profile);
        }
        for (name, instance) in made.instances {
            match instance {
                Some(instance) => drop(world.instances.insert(name, instance)),
                None => drop(world.instances.remove(&name)),
            }
        }
        for joint in made.joints_changed {
            match world.joints.iter_mut().find(|j| j.name == joint.name) {
                Some(was) => *was = joint,
                None => anyhow::bail!("there is no joint {} to change", joint.name),
            }
        }
        world.joints.extend(made.joints);
        world.couplings.extend(made.couplings);
        world.annotations.extend(made.annotations);
        world.threads.extend(made.threads);
        for (name, look) in made.looks {
            let now = match world.looks.get(&name) {
                Some(was) => look.over(was),
                None => look,
            };
            world.looks.insert(name, now);
        }
        world.views.extend(made.views);
        world.sections.extend(made.sections);
        world.masses.extend(made.masses);
        for (step, entries) in made.kept {
            self.cache_mut().replace(&step, entries);
        }
        Ok(())
    }

    /// Says that the step wrote the body `name`: made it, changed it or
    /// removed it.
    pub fn wrote(&self, name: &str) {
        if let Some(trace) = self.trace() {
            trace.wrote(name);
        }
    }

    /// The value of `measure` in the part being built; `what` names it in
    /// the error if its formula cannot be valued, or its tolerance cannot
    /// be held to (a tolerance class ISO 286 does not table).
    pub fn measure(&self, measure: &Measure, what: &str) -> anyhow::Result<f64> {
        use anyhow::Context;
        let value = measure
            .resolve(self.parameters())
            .with_context(|| what.to_owned())?;
        measure
            .tolerance
            .check(value)
            .map_err(|e| anyhow::anyhow!("{what}: {e}"))?;
        Ok(value)
    }
}

/// A lone world, with no parts to build: for applying operations outside a
/// workspace, in tests or previews.
#[derive(Debug, Clone, Default)]
pub struct Standalone {
    /// The world operations are applied to.
    pub world: World,
    /// What they worked out.
    pub cache: Cache,
    /// The parameters their formulas refer to.
    pub parameters: Parameters,
}

impl Standalone {
    /// A standalone builder over a fresh [`World`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl Builder for Standalone {
    fn world(&self) -> &World {
        &self.world
    }

    fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    fn cache(&self) -> &Cache {
        &self.cache
    }

    fn cache_mut(&mut self) -> &mut Cache {
        &mut self.cache
    }

    fn parameters(&self) -> &Parameters {
        &self.parameters
    }

    fn build(&self, path: &str, _: Option<&str>) -> anyhow::Result<Arc<World>> {
        anyhow::bail!("no part {path:?}: not building in a workspace")
    }
}
