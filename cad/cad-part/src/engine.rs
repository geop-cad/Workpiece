//! The [`Engine`]: what builds parts, and builds no more than it must.
//!
//! **Everything is keyed by what it is made from.** The world a part's
//! program has made after a step depends on the steps so far (their
//! arguments), the part's parameters, what the steps start solving from
//! (their entries of the part's cache), the parts they place (keyed the
//! same way, all the way down) and the files they read. Those are hashed
//! into a key, step after step; the world after every step is kept under
//! that step's key. A build looks for the longest stretch of its steps
//! that is kept, and only applies what comes after: a change to step 7
//! builds from step 6, a part placed twelve times is built once, and a
//! STEP file imported into a part of its own stays imported whatever
//! happens to the parts it is placed in.
//!
//! **Where instances are is not part of it.** The poses an assembly's
//! joints leave its parts at are worked out after the last step, on a
//! copy of the world that step made, and its [stamp](World::stamp) says
//! what it is apart from them: dragging a part changes poses and nothing
//! that is drawn has to be made again.
//!
//! **Parts build side by side.** Before a part's steps are applied, the
//! parts they place are started on a [`Pool`] of threads; a part is built
//! by one thread, a build that needs a part another thread is building
//! helps with other work until it is there. A [`Cancel`] stops a build
//! at the next [checkpoint](workpiece_core::cancel::checkpoint) the step
//! being applied passes; a step that panics is a step that failed.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use workpiece_core::{Builder, Cache, Parameters, World};

use crate::effect::{self, Effect};
use crate::naming;
use crate::pool::Pool;
use crate::{Part, Workspace, WorkspaceError};

/// What a world is made from, hashed.
pub type Key = u64;

/// How many worlds are kept.
const KEPT: usize = 512;

pub use workpiece_core::{Cancel, Cancelled};

/// What a build made: the world, and the part's cache as the build
/// leaves it (what its steps worked out, to start the next build from).
#[derive(Debug, Clone)]
pub struct Built {
    /// The world.
    pub world: Arc<World>,
    /// The cache.
    pub cache: Cache,
    /// The references that lost what they named and were pointed at
    /// what is now where that was: the part's program is to follow.
    pub repaired: Vec<naming::Repair>,
}

/// What a step did where it read `reads` of a world and they were
/// `input` (see [`effect::input`]).
#[derive(Clone)]
struct Met {
    reads: Arc<[workpiece_core::Read]>,
    input: u64,
    effect: Arc<Effect>,
}

/// How many ways of reading the world are kept of one step.
const WAYS: usize = 8;

struct Memo {
    kept: HashMap<Key, Built>,
    /// The keys kept, oldest first.
    order: VecDeque<Key>,
    /// The parts being built by some thread, by the key of their last
    /// step.
    running: HashSet<Key>,
    /// What steps did, by what they were and what they read (see
    /// [`crate::effect`]), and their keys, oldest first.
    effects: HashMap<Key, Vec<Met>>,
    effect_order: VecDeque<Key>,
    /// How many steps were applied (not found kept, nor their effects):
    /// for tests.
    applied: usize,
}

struct Inner {
    memo: Mutex<Memo>,
    pool: Option<Pool>,
}

/// Builds parts, keeping what it built. Cloning it shares what is kept.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Engine({:?})", self.inner.pool)
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::inline()
    }
}

use crate::pool::lock;

impl Engine {
    fn with(pool: Option<Pool>) -> Self {
        Self {
            inner: Arc::new(Inner {
                memo: Mutex::new(Memo {
                    kept: HashMap::new(),
                    order: VecDeque::new(),
                    running: HashSet::new(),
                    effects: HashMap::new(),
                    effect_order: VecDeque::new(),
                    applied: 0,
                }),
                pool,
            }),
        }
    }

    /// An engine that builds on the thread that asks.
    pub fn inline() -> Self {
        Self::with(None)
    }

    /// An engine that builds the parts a part places on `threads`
    /// threads beside the one that asks.
    pub fn threaded(threads: usize) -> Self {
        Self::with(Some(Pool::new(threads)))
    }

    /// The pool of threads, if it has one: for running whole builds off
    /// the thread that asks.
    pub fn pool(&self) -> Option<&Pool> {
        self.inner.pool.as_ref()
    }

    /// Builds `variant` of the part at `path` (the part as it is by
    /// default, for `None`) with the steps whose positions `keep` takes.
    pub fn build(
        &self,
        workspace: &Arc<Workspace>,
        path: &str,
        variant: Option<&str>,
        keep: &dyn Fn(usize) -> bool,
        cancel: &Cancel,
    ) -> anyhow::Result<Built> {
        Run {
            inner: self.inner.clone(),
            workspace: workspace.clone(),
            cancel: cancel.clone(),
            world: World::new(),
            cache: Cache::default(),
            trace: workpiece_core::Trace::default(),
            parameters: Parameters::new(),
            stack: Vec::new(),
            variant: None,
            keys: HashMap::new(),
            mine: None,
            own: HashMap::new(),
        }
        .part(path, variant, keep)
    }

    /// How many steps were applied so far, rather than found kept: for
    /// tests.
    pub fn applied(&self) -> usize {
        lock(&self.inner.memo).applied
    }

    /// How many worlds are kept: for tests.
    pub fn kept(&self) -> usize {
        lock(&self.inner.memo).kept.len()
    }
}

/// One build: a part being built, on top of those that place it.
struct Run {
    inner: Arc<Inner>,
    workspace: Arc<Workspace>,
    cancel: Cancel,
    /// The world being built: the innermost part on `stack`.
    world: World,
    /// What the step being applied reads of it.
    trace: workpiece_core::Trace,
    cache: Cache,
    parameters: Parameters,
    /// The paths of the parts being built, outermost first.
    stack: Vec<String>,
    /// The variant the innermost of them is built as.
    variant: Option<String>,
    /// The keys of whole parts worked out so far.
    keys: HashMap<(String, Option<String>), Key>,
    /// The part this run was started to build, by the key of its last
    /// step: the one it must not wait for.
    mine: Option<Key>,
    /// Of the part whose step keys were worked out last: what each of
    /// its steps is by itself (its arguments, what it starts solving
    /// from, the part or file it names), whatever came before it.
    own: HashMap<usize, Key>,
}

fn hash(of: impl Hash) -> Key {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    of.hash(&mut hasher);
    hasher.finish()
}

/// What a file is, as far as can be told without reading it.
fn file_stamp(path: &str) -> Key {
    workpiece_core::files::stamp(path).unwrap_or(0)
}

impl Run {
    fn resolve(&self, path: &str) -> Result<Cow<'_, Part>, WorkspaceError> {
        self.workspace.resolve(path)
    }

    /// The key of the whole part at `path`: of its last step. `above`
    /// are the parts that (would) place it, for finding a part that
    /// contains itself.
    fn key_of(
        &mut self,
        path: &str,
        variant: Option<&str>,
        above: &mut Vec<String>,
    ) -> anyhow::Result<Key> {
        let name = (path.to_owned(), variant.map(str::to_owned));
        if let Some(&key) = self.keys.get(&name) {
            return Ok(key);
        }
        let part = self.resolve(path)?.into_owned();
        let parameters = part
            .resolve(variant)
            .map_err(|e| e.context(format!("building part {path:?}")))?;
        let keys = self.step_keys(path, &part, &parameters, &part.cache, &|_| true, above)?;
        let key = keys
            .last()
            .map_or_else(|| hash(("empty", path, format!("{parameters:?}"))), |k| k.1);
        self.keys.insert(name, key);
        Ok(key)
    }

    /// The key after each kept step of `part`, with the step's position.
    fn step_keys(
        &mut self,
        path: &str,
        part: &Part,
        parameters: &Parameters,
        cache: &Cache,
        keep: &dyn Fn(usize) -> bool,
        above: &mut Vec<String>,
    ) -> anyhow::Result<Vec<(usize, Key)>> {
        if let Some(first) = above.iter().position(|p| p == path) {
            let mut cycle = above[first..].to_vec();
            cycle.push(path.into());
            return Err(WorkspaceError::Cycle(cycle).into());
        }
        above.push(path.to_owned());
        let mut key = hash(("part", format!("{parameters:?}"), &part.material));
        let mut out = Vec::new();
        let mut own = HashMap::new();
        let mut failed = None;
        let mut placing = String::new();
        for (i, op) in part.program.iter().enumerate().filter(|(i, _)| keep(*i)) {
            let text = serde_json::to_string(op).unwrap_or_default();
            // (Where a placed part is and whether a joint was made are
            // settled after the last step: no step starts from them.)
            let settled = op.settles();
            let solved: Vec<(&str, &[f64])> = if settled {
                Vec::new()
            } else {
                cache.of(op.id()).collect()
            };
            let mut solved = format!("{solved:?}");
            // A step after those that place and join sees what is placed
            // where its joints leave it, from where the part has it.
            if settled {
                let of: Vec<(&str, &[f64])> = cache.of(op.id()).collect();
                placing.push_str(&format!("{}{of:?}", op.id()));
            } else {
                solved.push_str(&placing);
            }
            // Where what it refers to was: what it finds it by, once its
            // name is lost (see [`naming`]).
            let refs = naming::owner(op.id());
            let was: Vec<(&str, &[f64])> = cache.of(&refs).collect();
            if !was.is_empty() {
                solved.push_str(&format!("{was:?}"));
            }
            // A step that refers to something of a placed part is built
            // against where that part is: it follows the part.
            for source in op.references() {
                if let Some(instance) = source.instance() {
                    let at: Vec<(&str, &[f64])> = cache.of(instance).collect();
                    solved.push_str(&format!("{instance}{at:?}"));
                }
            }
            // What it is built from besides the part: the part it places,
            // as that is; the files it reads, as they are.
            let needs = match op.placed() {
                Some((placed, variant)) => match self.key_of(placed, variant, above) {
                    Ok(key) => key,
                    Err(e) => {
                        failed = Some(e);
                        break;
                    }
                },
                None => op.files().first().map_or(0, |file| file_stamp(file)),
            };
            own.insert(i, hash((&text, &solved, needs)));
            key = hash((key, text, solved, needs));
            out.push((i, key));
        }
        self.own = own;
        above.pop();
        match failed {
            Some(e) => Err(e),
            None => Ok(out),
        }
    }

    fn kept(&self, key: Key) -> Option<Built> {
        lock(&self.inner.memo).kept.get(&key).cloned()
    }

    fn keep(&self, key: Key, built: &Built) {
        let mut memo = lock(&self.inner.memo);
        if memo.kept.insert(key, built.clone()).is_none() {
            memo.order.push_back(key);
        }
        while memo.order.len() > KEPT {
            if let Some(old) = memo.order.pop_front() {
                memo.kept.remove(&old);
            }
        }
    }

    /// Starts the parts the steps `ahead` place on the pool, those that
    /// are neither kept nor being built.
    fn start_placed(&mut self, part: &Part, ahead: &[usize]) {
        if self.inner.pool.is_none() {
            return;
        }
        let mut wanted: Vec<(String, Option<String>, Key)> = Vec::new();
        for &i in ahead {
            let Some((path, variant)) = part.program[i].placed() else {
                continue;
            };
            let (path, variant) = (path.to_owned(), variant.map(str::to_owned));
            let mut above = self.stack.clone();
            let Ok(key) = self.key_of(&path, variant.as_deref(), &mut above) else {
                continue;
            };
            if !wanted.iter().any(|w| w.2 == key) {
                wanted.push((path, variant, key));
            }
        }
        for (path, variant, key) in wanted {
            {
                let mut memo = lock(&self.inner.memo);
                if memo.kept.contains_key(&key) || !memo.running.insert(key) {
                    continue;
                }
            }
            let mut run = Run {
                inner: self.inner.clone(),
                workspace: self.workspace.clone(),
                cancel: self.cancel.clone(),
                world: World::new(),
                cache: Cache::default(),
                trace: workpiece_core::Trace::default(),
                parameters: Parameters::new(),
                stack: self.stack.clone(),
                variant: None,
                keys: self.keys.clone(),
                mine: Some(key),
                own: HashMap::new(),
            };
            let inner = self.inner.clone();
            let task = move || {
                // What fails here fails again, and is told, where the
                // part is placed.
                // (And however it ends, nobody waits for it after.)
                let cancel = run.cancel.clone();
                let _ = cancel.run(|| run.part(&path, variant.as_deref(), &|_| true));
                lock(&inner.memo).running.remove(&key);
            };
            if let Some(pool) = &self.inner.pool {
                pool.spawn(task);
            }
        }
    }

    /// Builds the part in a world of its own, on top of the one being
    /// built, which it leaves as it was: its steps, and then where its
    /// joints leave what it places.
    fn part(
        &mut self,
        path: &str,
        variant: Option<&str>,
        keep: &dyn Fn(usize) -> bool,
    ) -> anyhow::Result<Built> {
        let steps = self.steps(path, variant, keep)?;
        if steps.world.instances.is_empty() {
            return Ok(steps);
        }
        // Kept under the steps and what the part's cache has of where
        // its instances are: a drag changes only that.
        let part = self.resolve(path)?.into_owned();
        let settled_from = |cache: &Cache| {
            let mut entries = String::new();
            for op in &part.program {
                if op.settles() {
                    let of: Vec<(&str, &[f64])> = cache.of(op.id()).collect();
                    entries.push_str(&format!("{}{of:?}", op.id()));
                }
            }
            hash(("settled", steps.world.stamp, entries))
        };
        let key = settled_from(&part.cache);
        if let Some(built) = self.kept(key) {
            return Ok(built);
        }
        let mut world = (*steps.world).clone();
        // What the steps left, with where the part has its instances.
        let mut cache = steps.cache.clone();
        for op in &part.program {
            if op.settles() {
                let entries: Vec<(String, Vec<f64>)> = part
                    .cache
                    .of(op.id())
                    .map(|(name, values)| (name.to_owned(), values.to_vec()))
                    .collect();
                cache.replace(op.id(), entries);
            }
        }
        workpiece_ops_instances::assembly::settle(&mut world, &mut cache);
        let built = Built {
            world: Arc::new(world),
            cache,
            repaired: steps.repaired.clone(),
        };
        self.keep(key, &built);
        self.keep(settled_from(&built.cache), &built);
        Ok(built)
    }

    /// The world the part's steps make.
    fn steps(
        &mut self,
        path: &str,
        variant: Option<&str>,
        keep: &dyn Fn(usize) -> bool,
    ) -> anyhow::Result<Built> {
        let part = self.resolve(path)?.into_owned();
        let naming = |e: anyhow::Error| e.context(format!("building part {path:?}"));
        let parameters = part.resolve(variant).map_err(naming)?;
        let mut above = self.stack.clone();
        let keys = self.step_keys(path, &part, &parameters, &part.cache, keep, &mut above)?;
        let own = std::mem::take(&mut self.own);
        // (A part of no steps is kept too: every instance of it shares
        // one world.)
        let last = keys
            .last()
            .map_or_else(|| hash(("empty", path, format!("{parameters:?}"))), |k| k.1);
        // Another thread may be building just this: help until it has.
        if let Some(pool) = self.inner.pool.as_ref().filter(|_| self.mine != Some(last)) {
            let inner = &self.inner;
            pool.help_until(&|| !lock(&inner.memo).running.contains(&last));
        }
        if let Some(built) = self.kept(last) {
            return Ok(built);
        }
        // The longest stretch of the steps that is kept.
        let found = keys
            .iter()
            .enumerate()
            .rev()
            .find_map(|(n, (_, key))| Some((n, self.kept(*key)?)));
        let ahead: Vec<usize> = keys[found.as_ref().map_or(0, |f| f.0 + 1)..]
            .iter()
            .map(|k| k.0)
            .collect();
        self.stack.push(path.to_owned());
        let outer_variant = std::mem::replace(&mut self.variant, variant.map(str::to_owned));
        self.start_placed(&part, &ahead);

        let (world, cache) = match &found {
            Some((_, built)) => ((*built.world).clone(), {
                // What the kept steps worked out, over what the part
                // has for the steps still to come.
                let mut cache = part.cache.clone();
                for (i, _) in &keys[..=found.as_ref().map_or(0, |f| f.0)] {
                    // (Where parts are is the part's to say, not what a
                    // kept world was built with: it is no part of the
                    // key the world is kept under.)
                    if part.program[*i].settles() {
                        continue;
                    }
                    let id = part.program[*i].id();
                    let entries: Vec<(String, Vec<f64>)> = built
                        .cache
                        .of(id)
                        .map(|(name, values)| (name.to_owned(), values.to_vec()))
                        .collect();
                    cache.replace(id, entries);
                }
                cache
            }),
            None => (
                World {
                    material: part.material.clone(),
                    // (A part said not to be sectioned — a shaft, a pin —
                    // is drawn whole where a section runs through it.)
                    sectioned: match part.properties.get("Sectioned").map(String::as_str) {
                        Some("No") => workpiece_core::world::Sectioned::Whole,
                        _ => workpiece_core::world::Sectioned::Cut,
                    },
                    ..World::new()
                },
                part.cache.clone(),
            ),
        };
        let outer = (
            std::mem::replace(&mut self.world, world),
            std::mem::replace(&mut self.cache, cache),
            std::mem::replace(&mut self.parameters, parameters.clone()),
        );
        let mut result = Ok(());
        // (What the kept steps had found again, and what these do.)
        let mut repaired = found
            .as_ref()
            .map_or_else(Vec::new, |f| f.1.repaired.clone());
        // Whether what is placed may lie elsewhere than its joints
        // leave it.
        let mut unsettled = !self.world.instances.is_empty();
        for &i in &ahead {
            if self.cancel.is_cancelled() {
                result = Err(Cancelled.into());
                break;
            }
            // What it refers to and no longer finds by name, found by
            // where it was (see [`naming`]).
            let pointed = naming::settle(
                &part.program[i],
                &self.world,
                &mut self.cache,
                &mut repaired,
            );
            let op = pointed.as_ref().unwrap_or(&part.program[i]);
            // What is placed, where its joints leave it: a step after
            // the steps that place and join sees it there, as the last
            // step leaves it. (Worked out on a copy of the cache: what
            // the part keeps of it is settled after its last step.)
            if unsettled && !op.settles() {
                let mut scratch = self.cache.clone();
                workpiece_ops_instances::assembly::settle(&mut self.world, &mut scratch);
                unsettled = false;
            }
            let held = (self.world.instances.len(), self.world.joints.len());
            // The step as it is, with what it reads of the world as it
            // is now: if it met that before, what it did then is done
            // again, and the step is not applied.
            // (What it read is not known before it is applied: it is what
            // it read the times it was, looked at again.)
            let met = own
                .get(&i)
                .map(|own| hash((*own, format!("{parameters:?}"))));
            let ways = met.and_then(|key| lock(&self.inner.memo).effects.get(&key).cloned());
            let known = ways.unwrap_or_default().into_iter().find_map(|way| {
                (effect::input(&self.world, &way.reads) == way.input).then_some(way.effect)
            });
            if let Some(known) = known
                && let Ok(solved) = known.apply(&mut self.world)
            {
                self.cache.replace(op.id(), solved.to_vec());
            } else {
                let before = effect::Index::light(&self.world);
                // What it reads of the world as it is now, noted while
                // it is applied.
                let was = met.map(|_| self.world.clone());
                lock(&self.inner.memo).applied += 1;
                self.trace.take();
                self.world.topo.take_written();
                self.world.topo.watch();
                // (Stopped or broken, the world it was changing is
                // dropped with the build.)
                let cancel = self.cancel.clone();
                let applied = match cancel.run(|| op.apply(self)) {
                    Ok(applied) => applied,
                    Err(workpiece_core::Stopped::Cancelled) => Err(Cancelled.into()),
                    Err(broke) => Err(anyhow::anyhow!("{broke}")),
                };
                if let Err(e) = applied {
                    result = Err(e.context(format!("operation {:?}", op.id())));
                    break;
                }
                // The bodies it wrote: as the world noted, and as it said.
                // What it read: of the bodies, what the world saw it
                // look at; of the rest, what it refers to.
                let looked = self.world.topo.watched();
                (&*self as &dyn Builder).looked(looked);
                for source in op.references() {
                    if !matches!(source, workpiece_core::Source::Topology(_)) {
                        self.trace.note(workpiece_core::Read::Source(source));
                    }
                }
                let (reads, said) = self.trace.take();
                let mut wrote = self.world.topo.take_written();
                wrote.extend(said);
                let solved: Vec<(String, Vec<f64>)> = self
                    .cache
                    .of(op.id())
                    .map(|(name, values)| (name.to_owned(), values.to_vec()))
                    .collect();
                if let Some(key) = met
                    && let Some(was) = was
                    && let Some(done) = Effect::of(&before, &was, &self.world, &wrote, solved)
                    && done.settle(&mut self.world).is_ok()
                {
                    let way = Met {
                        input: effect::input(&was, &reads),
                        reads: reads.into(),
                        effect: Arc::new(done),
                    };
                    let mut memo = lock(&self.inner.memo);
                    let fresh = !memo.effects.contains_key(&key);
                    let ways = memo.effects.entry(key).or_default();
                    ways.retain(|w| w.reads != way.reads || w.input != way.input);
                    ways.insert(0, way);
                    ways.truncate(WAYS);
                    if fresh {
                        memo.effect_order.push_back(key);
                    }
                    while memo.effect_order.len() > KEPT {
                        if let Some(old) = memo.effect_order.pop_front() {
                            memo.effects.remove(&old);
                        }
                    }
                }
            }
            unsettled |=
                op.settles() || held != (self.world.instances.len(), self.world.joints.len());
            // The world after every step is kept, not only after the
            // last: going back through a part's steps (the scrubber,
            // an edit of an earlier step) builds nothing again.
            if let Some((_, key)) = keys.iter().find(|k| k.0 == i).filter(|k| k.1 != last) {
                let mut world = self.world.clone();
                world.stamp = *key;
                let step = Built {
                    world: Arc::new(world),
                    cache: self.cache.clone(),
                    repaired: repaired.clone(),
                };
                self.keep(*key, &step);
            }
        }
        self.stack.pop();
        self.variant = outer_variant;
        let mut world = std::mem::replace(&mut self.world, outer.0);
        let cache = std::mem::replace(&mut self.cache, outer.1);
        self.parameters = outer.2;
        result.map_err(naming)?;

        world.stamp = last;
        let built = Built {
            world: Arc::new(world),
            cache,
            repaired,
        };
        {
            self.keep(last, &built);
            // The same steps, started from what they just worked out,
            // make the same world: the next build asks for that.
            let mut above = self.stack.clone();
            if let Ok(again) =
                self.step_keys(path, &part, &parameters, &built.cache, keep, &mut above)
                && let Some((_, key)) = again.last()
            {
                self.keep(*key, &built);
            }
        }
        Ok(built)
    }
}

impl Builder for Run {
    fn world(&self) -> &World {
        self.trace.note(workpiece_core::Read::Everything);
        &self.world
    }

    fn world_mut(&mut self) -> &mut World {
        self.trace.note(workpiece_core::Read::Everything);
        &mut self.world
    }

    fn peek(&self) -> &World {
        &self.world
    }

    fn peek_mut(&mut self) -> &mut World {
        &mut self.world
    }

    fn trace(&self) -> Option<&workpiece_core::Trace> {
        Some(&self.trace)
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

    fn build(&self, path: &str, variant: Option<&str>) -> anyhow::Result<Arc<World>> {
        if let Some(first) = self.stack.iter().position(|p| p == path) {
            let mut cycle = self.stack[first..].to_vec();
            cycle.push(path.into());
            return Err(WorkspaceError::Cycle(cycle).into());
        }
        // (A build of its own, on top of the parts this one is built
        // on: nothing of this one changes.)
        let mut run = Run {
            inner: self.inner.clone(),
            workspace: self.workspace.clone(),
            cancel: self.cancel.clone(),
            world: World::new(),
            cache: Cache::default(),
            trace: workpiece_core::Trace::default(),
            parameters: Parameters::new(),
            stack: self.stack.clone(),
            variant: None,
            keys: self.keys.clone(),
            mine: None,
            own: HashMap::new(),
        };
        Ok(run.part(path, variant, &|_| true)?.world)
    }

    fn spanning(&self, steps: &[String]) -> anyhow::Result<(Arc<World>, Arc<World>)> {
        use anyhow::Context;
        self.trace.note(workpiece_core::Read::Everything);
        let (path, above) = self.stack.split_last().context("no part is being built")?;
        let part = self.resolve(path)?.into_owned();
        let mut span: Option<(usize, usize)> = None;
        for step in steps {
            let at = part
                .program
                .iter()
                .position(|op| op.id() == step)
                .with_context(|| format!("there is no step {step:?}"))?;
            span = Some(span.map_or((at, at), |(lo, hi)| (lo.min(at), hi.max(at))));
        }
        let (first, last) = span.context("no steps are named")?;
        // (Builds of their own of the part's first steps: kept ones, as
        // the part was just built through them.)
        let upto = |n: usize| -> anyhow::Result<Arc<World>> {
            let mut run = Run {
                inner: self.inner.clone(),
                workspace: self.workspace.clone(),
                cancel: self.cancel.clone(),
                world: World::new(),
                cache: Cache::default(),
                trace: workpiece_core::Trace::default(),
                parameters: Parameters::new(),
                stack: above.to_vec(),
                variant: None,
                keys: self.keys.clone(),
                mine: None,
                own: HashMap::new(),
            };
            Ok(run.steps(path, self.variant.as_deref(), &|i| i < n)?.world)
        };
        Ok((upto(first)?, upto(last + 1)?))
    }

    fn redone(&self, steps: &[String], on: World) -> anyhow::Result<World> {
        use anyhow::Context;
        self.trace.note(workpiece_core::Read::Everything);
        let path = self.stack.last().context("no part is being built")?;
        let part = self.resolve(path)?.into_owned();
        let mut span: Option<(usize, usize)> = None;
        for step in steps {
            let at = part
                .program
                .iter()
                .position(|op| op.id() == step)
                .with_context(|| format!("there is no step {step:?}"))?;
            span = Some(span.map_or((at, at), |(lo, hi)| (lo.min(at), hi.max(at))));
        }
        let (first, last) = span.context("no steps are named")?;
        // (A run of its own on the world given: nothing of this one
        // changes, and nothing of it is kept.)
        let mut run = Run {
            inner: self.inner.clone(),
            workspace: self.workspace.clone(),
            cancel: self.cancel.clone(),
            world: on,
            cache: Cache::default(),
            trace: workpiece_core::Trace::default(),
            parameters: self.parameters.clone(),
            stack: self.stack.clone(),
            variant: self.variant.clone(),
            keys: self.keys.clone(),
            mine: None,
            own: HashMap::new(),
        };
        for op in &part.program[first..=last] {
            op.apply(&mut run)
                .with_context(|| format!("operation {:?}, done again", op.id()))?;
        }
        Ok(run.world)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnyOperation;
    use std::collections::BTreeMap;
    use workpiece_core::world::ORIGIN;
    use workpiece_core::{Measure, Pose, Reference};
    use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs};
    use workpiece_ops_instances::{AddPart, AddPartArgs};

    fn block(id: &str, size: f64) -> AnyOperation {
        AddBox {
            id: id.into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(ORIGIN)),
                size: [size; 3].map(Measure::new),
            },
        }
        .into()
    }

    fn place(id: &str, part: &str) -> AnyOperation {
        AddPart {
            id: id.into(),
            args: AddPartArgs {
                part: part.into(),
                variant: None,
                hold: Default::default(),
                bodies: Vec::new(),
            },
        }
        .into()
    }

    /// An assembly placing `n` different blocks, each twice.
    fn workspace(n: usize) -> Workspace {
        let mut parts = BTreeMap::new();
        let mut program = Vec::new();
        for k in 0..n {
            let path = format!("block{k}");
            parts.insert(path.clone(), Part::new(vec![block("body", 1.0 + k as f64)]));
            program.push(place(&format!("a{k}"), &path));
            program.push(place(&format!("b{k}"), &path));
        }
        parts.insert("all".into(), Part::new(program));
        Workspace::new(parts)
    }

    fn build(engine: &Engine, ws: &Workspace, path: &str) -> anyhow::Result<Built> {
        engine.build(&Arc::new(ws.clone()), path, None, &|_| true, &Cancel::new())
    }

    #[test]
    fn features_are_repeated_by_what_their_steps_did() -> anyhow::Result<()> {
        use workpiece_core::math::Axis;
        use workpiece_core::{Source, world::topo::BodyId};
        use workpiece_ops_bodies::{Along, Pattern, PatternArgs};
        use workpiece_ops_features::{Hole, HoleArgs, HolePlace};
        // A block 4 wide with a hole ⌀0.5 through it, 1 in from two of
        // its sides.
        let hole = Hole {
            id: "hole".into(),
            args: HoleArgs {
                place: HolePlace::Offsets,
                face: Some(Source::Topology("body/top".into()).into()),
                first: Some(Source::Topology("body/right".into()).into()),
                first_offset: Measure::new(1.0),
                second: Some(Source::Topology("body/back".into()).into()),
                second_offset: Measure::new(1.0),
                diameter: Measure::new(0.5),
                extent: workpiece_ops_sweep::Extent::ThroughAll,
                ..Default::default()
            },
        };
        let engine = Engine::inline();
        let mut ws = Workspace::new(BTreeMap::from([(
            "plate".to_owned(),
            Part::new(vec![block("body", 4.0), hole.into()]),
        )]));
        let volume = |built: &Built| -> anyhow::Result<f64> {
            let topo = &built.world.topo;
            let body = topo.resolve::<BodyId>("body").expect("the body");
            Ok(workpiece_boolean::measure(&workpiece_boolean::read(topo, body)?).volume)
        };
        let one = std::f64::consts::PI * 0.25 * 0.25 * 4.0;
        let drilled = build(&engine, &ws, "plate")?;
        assert!((volume(&drilled)? - (64.0 - one)).abs() < 1e-6);
        // The hole, four times around the axis through the middle: a
        // face of it says which feature.
        let face = drilled
            .world
            .topo
            .names()
            .with_prefix("hole")
            .map(|(name, _)| name.to_owned())
            .find(|name| {
                let topo = &drilled.world.topo;
                topo.resolve::<workpiece_core::world::topo::FaceId>(name)
                    .is_some()
            })
            .expect("a face of the hole");
        let pattern = Pattern {
            id: "around".into(),
            args: PatternArgs {
                features: vec![Source::Topology(face).into()],
                along: Along::Circle,
                axis: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
                count: 4,
                ..Default::default()
            },
        };
        let part = ws.parts.get_mut("plate").expect("the part");
        part.program.push(pattern.into());
        let all = build(&engine, &ws, "plate")?;
        assert_eq!(all.world.topo.validate(), Ok(()));
        assert!((volume(&all)? - (64.0 - 4.0 * one)).abs() < 1e-6);
        Ok(())
    }

    /// A block 4 wide (`body`) with a hole ⌀0.5 through it 1 in from
    /// two of its sides, and the name of a face of the hole; and the
    /// volume of the hole.
    fn drilled_block(
        engine: &Engine,
    ) -> anyhow::Result<(Vec<AnyOperation>, String, workpiece_core::math::Point3, f64)> {
        use workpiece_core::Source;
        use workpiece_ops_features::{Hole, HoleArgs, HolePlace};
        let hole = Hole {
            id: "hole".into(),
            args: HoleArgs {
                place: HolePlace::Offsets,
                face: Some(Source::Topology("body/top".into()).into()),
                first: Some(Source::Topology("body/right".into()).into()),
                first_offset: Measure::new(1.0),
                second: Some(Source::Topology("body/back".into()).into()),
                second_offset: Measure::new(1.0),
                diameter: Measure::new(0.5),
                extent: workpiece_ops_sweep::Extent::ThroughAll,
                ..Default::default()
            },
        };
        let program = vec![block("body", 4.0), hole.into()];
        let ws = Workspace::new(BTreeMap::from([(
            "plate".to_owned(),
            Part::new(program.clone()),
        )]));
        let built = build(engine, &ws, "plate")?;
        let topo = &built.world.topo;
        let body = topo
            .resolve::<workpiece_core::world::topo::BodyId>("body")
            .expect("the body");
        let solid = workpiece_boolean::read(topo, body)?;
        let wall = solid
            .faces
            .iter()
            .find(|f| f.name.as_deref().is_some_and(|n| n.starts_with("hole")))
            .expect("the hole's wall");
        // (Where its axis is: the middle of its wall's points.)
        let points = wall.surface.control_points();
        let middle =
            points.iter().copied().sum::<workpiece_core::math::Point3>() / points.len() as f64;
        let name = wall.name.clone().expect("named");
        Ok((
            program,
            name,
            middle,
            std::f64::consts::PI * 0.25 * 0.25 * 4.0,
        ))
    }

    #[test]
    fn a_feature_goes_to_each_point_of_a_sketch_wherever_it_is() -> anyhow::Result<()> {
        // In practic1, a pattern of a hole at the one point of a sketch
        // failed ("fewer than two points"), and one at four points, none
        // of them at the hole, put the copies off the part: the hole was
        // taken to be at the point nearest it. It is moved to each point
        // from where it is, unless it is at one already.
        use workpiece_core::math::{Axis, Point2};
        use workpiece_core::{Source, world::topo::BodyId};
        use workpiece_ops_bodies::{Along, Pattern, PatternArgs};
        let engine = Engine::inline();
        let (program, wall, at, one) = drilled_block(&engine)?;
        let volume = |points: &[[f64; 2]]| -> anyhow::Result<f64> {
            let mut sketch = workpiece_ops_sketch::Sketch::new("spots");
            sketch.args.plane = Some(Reference::frame_axis(ORIGIN, Axis::Z));
            for p in points {
                let p = sketch.args.add_point(Point2::from_array(*p));
                sketch.args.add(workpiece_ops_sketch::Shape::Point(p));
            }
            let pattern = Pattern {
                id: "spread".into(),
                args: PatternArgs {
                    features: vec![Source::Topology(wall.clone()).into()],
                    along: Along::Points,
                    sketch: Some(Source::Profile("spots".into()).into()),
                    ..Default::default()
                },
            };
            let mut all = program.clone();
            all.extend([sketch.into(), pattern.into()]);
            let ws = Workspace::new(BTreeMap::from([("plate".to_owned(), Part::new(all))]));
            let built = build(&engine, &ws, "plate")?;
            assert_eq!(built.world.topo.validate(), Ok(()));
            let topo = &built.world.topo;
            let body = topo.resolve::<BodyId>("body").expect("the body");
            Ok(workpiece_boolean::measure(&workpiece_boolean::read(topo, body)?).volume)
        };
        // One point, away from the hole: a second hole there.
        assert!((volume(&[[-1.0, -1.0]])? - (64.0 - 2.0 * one)).abs() < 1e-6);
        // Three, none at the hole: three more.
        let three = volume(&[[-1.0, -1.0], [0.0, 0.0], [0.0, -1.0]])?;
        assert!((three - (64.0 - 4.0 * one)).abs() < 1e-6, "{three}");
        // Two, one of them at the hole: one more.
        let two = volume(&[[at.x, at.y], [-1.0, -1.0]])?;
        assert!((two - (64.0 - 2.0 * one)).abs() < 1e-6, "{two}");
        Ok(())
    }

    #[test]
    fn a_hole_fills_a_face_as_far_as_the_margin() -> anyhow::Result<()> {
        use workpiece_core::{Source, world::topo::BodyId};
        use workpiece_ops_bodies::{Along, Grid, Pattern, PatternArgs};
        let engine = Engine::inline();
        let (mut program, wall, _, one) = drilled_block(&engine)?;
        // A square grid 1 apart from the hole at (1, 1) over the top,
        // each copy a margin of 0.25 in from the edges besides its own
        // radius: x and y of −1, 0 and 1.
        program.push(
            Pattern {
                id: "grid".into(),
                args: PatternArgs {
                    features: vec![Source::Topology(wall).into()],
                    along: Along::Fill,
                    face: Some(Source::Topology("body/top".into()).into()),
                    grid: Grid::Square,
                    spacing: Measure::new(1.0),
                    margin: Measure::new(0.25),
                    ..Default::default()
                },
            }
            .into(),
        );
        let ws = Workspace::new(BTreeMap::from([("plate".to_owned(), Part::new(program))]));
        let built = build(&engine, &ws, "plate")?;
        let topo = &built.world.topo;
        let body = topo.resolve::<BodyId>("body").expect("the body");
        let volume = workpiece_boolean::measure(&workpiece_boolean::read(topo, body)?).volume;
        assert!((volume - (64.0 - 9.0 * one)).abs() < 1e-6, "{volume}");
        Ok(())
    }

    #[test]
    fn a_cut_repeated_cuts_what_its_tool_meets_there() -> anyhow::Result<()> {
        use workpiece_core::math::{Axis, Point2};
        use workpiece_core::{Source, world::topo::BodyId};
        use workpiece_ops_bodies::{Along, Pattern, PatternArgs};
        use workpiece_ops_sweep::{Extent, Extrude};
        // A block 4 across around the origin, and a slot 1 wide cut
        // through it from x = 1 to x = 3: half of the slot's outline is
        // past the block, so only 1 × 1 × 4 is cut.
        let mut sketch = workpiece_ops_sketch::Sketch::new("s");
        sketch.args.plane = Some(Reference::frame_axis(ORIGIN, Axis::Z));
        let corners = [[1.0, -0.5], [3.0, -0.5], [3.0, 0.5], [1.0, 0.5]]
            .map(|p| sketch.args.add_point(Point2::from_array(p)));
        for k in 0..4 {
            sketch.args.add_line(corners[k], corners[(k + 1) % 4]);
        }
        let mut slot = Extrude::of("slot", Some("s".into()), false);
        slot.args.combine = serde_json::from_str("\"Cut\"")?;
        slot.args.extent = Extent::ThroughBoth;
        // Again 2 to the left: all of its outline is in the block
        // there, and all of it is cut — not a copy of the half.
        let pattern = Pattern {
            id: "again".into(),
            args: PatternArgs {
                features: vec![Source::Topology("slot/side/e7".into()).into()],
                along: Along::Line,
                axis: Some(Reference::frame_axis(ORIGIN, Axis::X)),
                count: 2,
                spacing: Measure::new(-2.0),
                ..Default::default()
            },
        };
        let engine = Engine::inline();
        let mut ws = Workspace::new(BTreeMap::from([(
            "part".to_owned(),
            Part::new(vec![block("body", 4.0), sketch.into(), slot.into()]),
        )]));
        let volume = |built: &Built| -> anyhow::Result<f64> {
            let topo = &built.world.topo;
            let body = topo.resolve::<BodyId>("body").expect("the body");
            Ok(workpiece_boolean::measure(&workpiece_boolean::read(topo, body)?).volume)
        };
        let once = build(&engine, &ws, "part")?;
        assert!((volume(&once)? - (64.0 - 4.0)).abs() < 1e-9);
        // (A face of the slot says which feature.)
        let face = once
            .world
            .topo
            .names()
            .with_prefix("slot")
            .map(|(name, _)| name.to_owned())
            .find(|name| {
                let topo = &once.world.topo;
                topo.resolve::<workpiece_core::world::topo::FaceId>(name)
                    .is_some()
            })
            .expect("a face of the slot");
        let mut pattern = pattern;
        pattern.args.features = vec![Source::Topology(face).into()];
        ws.part_mut("part")?.program.push(pattern.into());
        let twice = build(&engine, &ws, "part")?;
        assert_eq!(twice.world.topo.validate(), Ok(()));
        assert!((volume(&twice)? - (64.0 - 4.0 - 8.0)).abs() < 1e-9);
        Ok(())
    }

    /// A rectangle `[u0, v0, u1, v1]` sketched on `plane`, called `id`.
    fn rectangle(
        id: &str,
        plane: Reference<workpiece_core::world::DatumPlane>,
        r: [f64; 4],
    ) -> AnyOperation {
        use workpiece_core::math::Point2;
        let mut sketch = workpiece_ops_sketch::Sketch::new(id);
        sketch.args.plane = Some(plane);
        let corners = [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]]
            .map(|p| sketch.args.add_point(Point2::from_array(p)));
        for k in 0..4 {
            sketch.args.add_line(corners[k], corners[(k + 1) % 4]);
        }
        sketch.into()
    }

    /// An extrusion of the sketch `of`, `by` far, cutting or joining.
    fn extruded(id: &str, of: &str, by: f64, cut: bool) -> AnyOperation {
        let mut extrude = workpiece_ops_sweep::Extrude::of(id, Some(of.into()), false);
        extrude.args.distance = Measure::new(by);
        extrude.args.combine = serde_json::from_str(if cut { "\"Cut\"" } else { "\"Join\"" })
            .expect("a way to combine");
        extrude.into()
    }

    /// A fillet of radius `r` of all edges of the face `face`.
    fn rounded(id: &str, face: &str, r: f64) -> AnyOperation {
        use workpiece_ops_blend::{Fillet, FilletArgs};
        Fillet {
            id: id.into(),
            args: FilletArgs {
                edges: vec![workpiece_core::Source::Topology(face.to_owned()).into()],
                size: Measure::new(r),
                ..Default::default()
            },
        }
        .into()
    }

    /// The name of a face of the step `step` all of whose points have
    /// `along` (a coordinate of 0, 1 or 2) equal to `at`.
    fn flat_at(built: &Built, step: &str, along: usize, at: f64) -> String {
        let topo = &built.world.topo;
        let body = topo
            .resolve::<workpiece_core::world::topo::BodyId>("body")
            .expect("the body");
        let solid = workpiece_boolean::read(topo, body).expect("a solid");
        let face = solid.faces.iter().find(|f| {
            f.name.as_deref().is_some_and(|n| n.starts_with(step))
                && f.surface
                    .control_points()
                    .iter()
                    .all(|p| (p[along] - at).abs() < 1e-9)
        });
        face.and_then(|f| f.name.clone()).expect("a flat face")
    }

    /// The name of a face of the step `step`.
    fn face_of(built: &Built, step: &str) -> String {
        let topo = &built.world.topo;
        let body = topo
            .resolve::<workpiece_core::world::topo::BodyId>("body")
            .expect("the body");
        let solid = workpiece_boolean::read(topo, body).expect("a solid");
        let face = solid
            .faces
            .iter()
            .find(|f| f.name.as_deref().is_some_and(|n| n.starts_with(step)));
        face.and_then(|f| f.name.clone())
            .expect("a face of the step")
    }

    fn body_volume(built: &Built) -> f64 {
        let topo = &built.world.topo;
        let body = topo
            .resolve::<workpiece_core::world::topo::BodyId>("body")
            .expect("the body");
        workpiece_boolean::measure(&workpiece_boolean::read(topo, body).expect("a solid")).volume
    }

    fn mirrored(
        id: &str,
        faces: &[&str],
        plane: Reference<workpiece_core::world::DatumPlane>,
    ) -> AnyOperation {
        use workpiece_ops_bodies::{Mirror, MirrorArgs};
        Mirror {
            id: id.into(),
            args: MirrorArgs {
                features: faces
                    .iter()
                    .map(|f| workpiece_core::Source::Topology((*f).to_owned()).into())
                    .collect(),
                plane: Some(plane),
                ..Default::default()
            },
        }
        .into()
    }

    fn same_solid(a: &Built, b: &Built) {
        assert_eq!(a.world.topo.validate(), Ok(()));
        assert_eq!(b.world.topo.validate(), Ok(()));
        let (va, vb) = (body_volume(a), body_volume(b));
        assert!((va - vb).abs() < 1e-9, "{va} against {vb}");
    }

    #[test]
    fn a_pocket_and_its_fillet_mirrored_are_two_pockets_by_hand() -> anyhow::Result<()> {
        use workpiece_core::math::Axis;
        let top = || Reference::<workpiece_core::world::DatumPlane>::topology("body/top");
        let engine = Engine::inline();
        let mut by_hand = Workspace::new(BTreeMap::from([(
            "part".to_owned(),
            Part::new(vec![
                block("body", 4.0),
                rectangle("s1", top(), [0.5, 0.5, 1.5, 1.5]),
                extruded("pocket", "s1", 1.0, true),
            ]),
        )]));
        let one = build(&engine, &by_hand, "part")?;
        assert!((body_volume(&one) - 63.0).abs() < 1e-9);
        let floor = flat_at(&one, "pocket", 2, 1.0);
        // The two pockets, each with its fillet, drawn by hand: u and
        // v are x and y of the top face, as the sketch frame lays them
        // out, so a mirror in y is v → −v.
        let program = &mut by_hand.part_mut("part")?.program;
        program.push(rounded("round", &floor, 0.2));
        program.push(rectangle("s2", top(), [0.5, -1.5, 1.5, -0.5]));
        program.push(extruded("pocket2", "s2", 1.0, true));
        let floor2 = flat_at(&build(&engine, &by_hand, "part")?, "pocket2", 2, 1.0);
        by_hand
            .part_mut("part")?
            .program
            .push(rounded("round2", &floor2, 0.2));
        let by_hand = build(&engine, &by_hand, "part")?;
        // The same with the features told once and mirrored.
        let once = Workspace::new(BTreeMap::from([(
            "part".to_owned(),
            Part::new(vec![
                block("body", 4.0),
                rectangle("s1", top(), [0.5, 0.5, 1.5, 1.5]),
                extruded("pocket", "s1", 1.0, true),
                rounded("round", &floor, 0.2),
            ]),
        )]));
        let mut mirror = once.clone();
        let round = face_of(&build(&engine, &once, "part")?, "round");
        mirror.part_mut("part")?.program.push(mirrored(
            "mirror",
            &[&floor, &round],
            Reference::frame_axis(ORIGIN, Axis::Y),
        ));
        let mirror = build(&engine, &mirror, "part")?;
        same_solid(&by_hand, &mirror);
        assert!(body_volume(&mirror) < 62.2);
        // What the steps made there is named under the mirror's id.
        let names = mirror.world.topo.names();
        assert!(names.with_prefix("mirror/pocket").next().is_some());
        assert!(names.with_prefix("mirror/round").next().is_some());
        Ok(())
    }

    #[test]
    fn a_counterbored_hole_mirrored_is_a_hole_by_hand() -> anyhow::Result<()> {
        use workpiece_core::math::Axis;
        use workpiece_core::{Source, world::topo::FaceId};
        use workpiece_ops_features::{Hole, HoleArgs, HoleKind, HolePlace};
        let hole = |id: &str, side: &str| -> AnyOperation {
            Hole {
                id: id.into(),
                args: HoleArgs {
                    place: HolePlace::Offsets,
                    face: Some(Source::Topology("body/top".into()).into()),
                    first: Some(Source::Topology(format!("body/{side}")).into()),
                    first_offset: Measure::new(1.0),
                    second: Some(Source::Topology("body/back".into()).into()),
                    second_offset: Measure::new(1.0),
                    diameter: Measure::new(0.5),
                    kind: HoleKind::Counterbore,
                    head: Measure::new(1.0),
                    head_depth: Measure::new(0.5),
                    extent: workpiece_ops_sweep::Extent::ThroughAll,
                    ..Default::default()
                },
            }
            .into()
        };
        let engine = Engine::inline();
        let part =
            |program| Workspace::new(BTreeMap::from([("part".to_owned(), Part::new(program))]));
        let once = build(
            &engine,
            &part(vec![block("body", 4.0), hole("hole", "right")]),
            "part",
        )?;
        let both = build(
            &engine,
            &part(vec![
                block("body", 4.0),
                hole("hole", "right"),
                hole("other", "left"),
            ]),
            "part",
        )?;
        let face = face_of(&once, "hole");
        let mut ws = part(vec![block("body", 4.0), hole("hole", "right")]);
        ws.part_mut("part")?.program.push(mirrored(
            "mirror",
            &[&face],
            Reference::frame_axis(ORIGIN, Axis::Y),
        ));
        let mirror = build(&engine, &ws, "part")?;
        assert!(mirror.world.topo.resolve::<FaceId>(&face).is_some());
        assert!(body_volume(&mirror) < body_volume(&once) - 1.0);
        same_solid(&both, &mirror);
        Ok(())
    }

    #[test]
    fn a_boss_mirrored_about_a_flat_face_is_a_boss_by_hand() -> anyhow::Result<()> {
        // A boss on the top face reaching 0.5 past the right face and
        // so out over it, mirrored in that face (a plane 2 from the
        // middle): a second boss from 1.5 to 3.5, which joins the first.
        let top = || Reference::<workpiece_core::world::DatumPlane>::topology("body/top");
        let engine = Engine::inline();
        let part =
            |program| Workspace::new(BTreeMap::from([("part".to_owned(), Part::new(program))]));
        let first = |id: &str| -> Vec<AnyOperation> {
            vec![
                block("body", 4.0),
                rectangle("s1", top(), [-1.0, 0.5, 1.0, 2.5]),
                extruded(id, "s1", 1.0, false),
            ]
        };
        let one = build(&engine, &part(first("boss")), "part")?;
        assert!((body_volume(&one) - (64.0 + 4.0)).abs() < 1e-9);
        let mut by_hand = first("boss");
        by_hand.push(rectangle("s2", top(), [-1.0, 1.5, 1.0, 3.5]));
        by_hand.push(extruded("boss2", "s2", 1.0, false));
        let by_hand = build(&engine, &part(by_hand), "part")?;
        let face = face_of(&one, "boss");
        let mut ws = part(first("boss"));
        ws.part_mut("part")?.program.push(mirrored(
            "mirror",
            &[&face],
            Reference::topology("body/right"),
        ));
        let mirror = build(&engine, &ws, "part")?;
        assert!(body_volume(&mirror) > body_volume(&one) + 1.0);
        same_solid(&by_hand, &mirror);
        Ok(())
    }

    #[test]
    fn what_did_not_change_is_not_built_again() -> anyhow::Result<()> {
        let engine = Engine::inline();
        let mut ws = workspace(3);
        let first = build(&engine, &ws, "all")?;
        // The same again: the very same world.
        let again = build(&engine, &ws, "all")?;
        assert!(Arc::ptr_eq(&first.world, &again.world));
        // One block changed: the others' worlds are the ones kept.
        ws.parts
            .insert("block1".into(), Part::new(vec![block("body", 9.0)]));
        let changed = build(&engine, &ws, "all")?;
        let world = |b: &Built, name: &str| b.world.instances[name].world.clone();
        assert!(Arc::ptr_eq(&world(&first, "a0"), &world(&changed, "a0")));
        assert!(Arc::ptr_eq(&world(&first, "b2"), &world(&changed, "b2")));
        assert!(!Arc::ptr_eq(&world(&first, "a1"), &world(&changed, "a1")));
        // Both instances of a part share one world.
        assert!(Arc::ptr_eq(&world(&changed, "a1"), &world(&changed, "b1")));
        Ok(())
    }

    #[test]
    fn a_step_added_builds_from_the_steps_before_it() -> anyhow::Result<()> {
        let engine = Engine::inline();
        let mut ws = Workspace::new(BTreeMap::from([(
            "part".to_owned(),
            Part::new(vec![block("one", 1.0), block("two", 2.0)]),
        )]));
        let two = build(&engine, &ws, "part")?;
        let kept = engine.kept();
        ws.part_mut("part")?.program.push(block("three", 3.0));
        let three = build(&engine, &ws, "part")?;
        assert!(three.world.topo.lookup("three").is_some());
        assert!(three.world.topo.lookup("one").is_some());
        // Only the new end was added to what is kept (under the key it
        // was asked by, and the key of what it worked out).
        assert!(engine.kept() <= kept + 2, "{} then {}", kept, engine.kept());
        // What is drawn of a world is told apart by its stamp.
        assert_ne!(two.world.stamp, three.world.stamp);
        assert_ne!(three.world.stamp, 0);
        Ok(())
    }

    #[test]
    fn what_a_step_refers_to_is_found_where_it_was_once_its_name_is_lost() -> anyhow::Result<()> {
        use workpiece_core::world::topo::EntityId;
        use workpiece_ops_blend::{Fillet, FilletArgs};
        let round = |edge: &str| -> AnyOperation {
            Fillet {
                id: "round".into(),
                args: FilletArgs {
                    edges: vec![workpiece_core::Source::Topology(edge.to_owned()).into()],
                    size: workpiece_core::Measure::new(0.1),
                    ..Default::default()
                },
            }
            .into()
        };
        // Some edge of the block, rounded: where it is is kept.
        let engine = Engine::inline();
        let block_alone = Workspace::new(BTreeMap::from([(
            "part".to_owned(),
            Part::new(vec![block("one", 1.0)]),
        )]));
        let alone = build(&engine, &block_alone, "part")?;
        let names = alone.world.topo.names();
        let edge = names.iter().find(|(_, id)| matches!(id, EntityId::Edge(_)));
        let edge = edge.map(|(name, _)| name.to_owned()).expect("an edge");
        let mut ws = Workspace::new(BTreeMap::from([(
            "part".to_owned(),
            Part::new(vec![block("one", 1.0), round(&edge)]),
        )]));
        let first = build(&engine, &ws, "part")?;
        let kept = first.cache.get(&format!("round@refs/{edge}"));
        let kept = kept.expect("where the edge is").to_vec();
        let faces = |built: &Built| built.world.topo.faces().len();
        assert_eq!(faces(&first), 7);
        // What the round made has no name of its own: its edges are
        // called by the faces they lie between, not by a number that
        // another edge upstream would shift.
        let made: Vec<&str> = first
            .world
            .topo
            .names()
            .iter()
            .map(|(name, _)| name)
            .collect();
        let between = made.iter().filter(|n| n.starts_with("one/edge[")).count();
        assert!(between >= 4, "{made:?}");
        assert!(made.iter().any(|n| n.contains("+round/")), "{made:?}");
        // The edge under a name that is no more (as after a change
        // upstream that makes it another way): it is found where it
        // was, and rounded as before.
        let part = ws.part_mut("part")?;
        part.program[1] = round("one/gone");
        part.cache = first.cache.clone();
        part.cache
            .replace("round@refs", [("one/gone".to_owned(), kept)]);
        let second = build(&engine, &ws, "part")?;
        assert_eq!(faces(&second), 7);
        // The build says so: the program is to refer to it from now on.
        let [repair] = &second.repaired[..] else {
            anyhow::bail!("expected the edge found again");
        };
        assert_eq!(repair.step, "round");
        assert_eq!(repair.to, workpiece_core::Source::Topology(edge.clone()));
        assert!(first.repaired.is_empty());
        // With nothing kept of where it was, it is lost: never guessed.
        ws.part_mut("part")?.cache = Cache::default();
        let lost = build(&engine, &ws, "part")
            .map(|_| ())
            .expect_err("no such edge");
        assert!(format!("{lost:#}").contains("one/gone"), "{lost:#}");
        Ok(())
    }

    #[test]
    fn what_cuts_through_parts_reads_only_those_it_could_run_into() -> anyhow::Result<()> {
        use workpiece_ops_instances::{CutParts, CutPartsArgs};
        let cut: AnyOperation = CutParts {
            id: "cut".into(),
            args: CutPartsArgs {
                tools: vec![workpiece_core::Source::Topology("tool".into()).into()],
            },
        }
        .into();
        // A part where the tool is, and one far from it.
        let assembly = Part::new(vec![
            block("tool", 0.5),
            place("near", "near"),
            place("far", "far"),
            cut,
        ])
        .placing(
            "far",
            &Pose::from_translation(workpiece_core::math::Point3::X * 10.0),
        );
        let engine = Engine::inline();
        let mut ws = Workspace::new(BTreeMap::from([
            ("near".to_owned(), Part::new(vec![block("a", 1.0)])),
            ("far".to_owned(), Part::new(vec![block("a", 1.0)])),
            ("all".to_owned(), assembly),
        ]));
        build(&engine, &ws, "all")?;
        let first = engine.applied();
        // Something more inside the far part, which reaches no further
        // for it: the far part is built and placed again, but nothing is
        // cut again.
        ws.part_mut("far")?.program.push(block("b", 0.5));
        build(&engine, &ws, "all")?;
        let far = engine.applied() - first;
        // The same inside the near part: it is cut again too.
        ws.part_mut("near")?.program.push(block("b", 0.4));
        build(&engine, &ws, "all")?;
        let near = engine.applied() - first - far;
        assert_eq!(near, far + 1, "{far} then {near}");
        Ok(())
    }

    #[test]
    fn a_step_is_applied_again_only_if_what_it_reads_changed() -> anyhow::Result<()> {
        use workpiece_ops_blend::{Fillet, FilletArgs};
        let round = |body: &str| -> AnyOperation {
            Fillet {
                id: format!("round-{body}"),
                args: FilletArgs {
                    edges: vec![workpiece_core::Source::Topology(body.to_owned()).into()],
                    mitered: true,
                    ..Default::default()
                },
            }
            .into()
        };
        let engine = Engine::inline();
        let mut ws = Workspace::new(BTreeMap::from([(
            "part".to_owned(),
            Part::new(vec![block("one", 1.0), block("two", 2.0), round("two")]),
        )]));
        let first = build(&engine, &ws, "part")?;
        assert_eq!(engine.applied(), 3);
        // The first block changes: the second is made of nothing of it,
        // and its rounds are of the second alone. Neither is applied
        // again, and the world is as if they had been.
        ws.part_mut("part")?.program[0] = block("one", 1.5);
        let second = build(&engine, &ws, "part")?;
        assert_eq!(engine.applied(), 4, "only the changed block");
        let faces = |built: &Built, body: &str| {
            let topo = &built.world.topo;
            let id = topo.resolve::<workpiece_core::world::topo::BodyId>(body);
            id.and_then(|b| topo.body_faces(b)).map(|f| f.len())
        };
        assert_eq!(faces(&second, "two"), faces(&first, "two"));
        assert_eq!(faces(&second, "one"), Some(6));
        assert_eq!(second.world.topo.validate(), Ok(()));
        // A step moved to another place among the others, with the same
        // to read there, is not applied again either.
        ws.part_mut("part")?.program.swap(0, 1);
        let third = build(&engine, &ws, "part")?;
        assert_eq!(engine.applied(), 4);
        assert_eq!(faces(&third, "two"), faces(&first, "two"));
        // The rounds are made again when their body changes.
        ws.part_mut("part")?.program[0] = block("two", 2.5);
        build(&engine, &ws, "part")?;
        assert_eq!(engine.applied(), 6, "the block and its rounds");
        Ok(())
    }

    #[test]
    fn where_parts_are_does_not_change_what_they_are() -> anyhow::Result<()> {
        let engine = Engine::inline();
        let mut ws = workspace(2);
        let first = build(&engine, &ws, "all")?;
        // The part moved by a drag: only its place in the cache changes.
        let moved = Pose::from_translation(workpiece_core::math::Point3::new(5.0, 0.0, 0.0));
        let put = BTreeMap::from([("a0".to_owned(), moved)]);
        workpiece_ops_instances::assembly::keep(&mut ws.part_mut("all")?.cache, &put);
        let second = build(&engine, &ws, "all")?;
        assert_eq!(second.world.instances["a0"].pose, moved);
        assert_eq!(second.world.instances["b0"].pose, Pose::IDENTITY);
        // The same worlds placed, and the same stamp: nothing to draw
        // anew.
        assert!(Arc::ptr_eq(
            &first.world.instances["a0"].world,
            &second.world.instances["a0"].world
        ));
        assert_eq!(first.world.stamp, second.world.stamp);
        Ok(())
    }

    #[test]
    fn a_step_can_stand_on_a_placed_part_and_follows_it() -> anyhow::Result<()> {
        use workpiece_core::Source;
        use workpiece_core::world::Datum;
        use workpiece_ops_datum::{AddPlane, AddPlaneArgs, PlaneMode};
        // A plane a little above the top of a block placed in the part.
        let top = Source::placed(&["a".to_owned()], Source::Topology("body/top".into()));
        let plane = AddPlane {
            id: "above".into(),
            args: AddPlaneArgs {
                mode: PlaneMode::Offset,
                references: vec![top.into()],
                distance: 2.0,
                ..AddPlaneArgs::default()
            },
        };
        let mut ws = Workspace::new(BTreeMap::from([
            ("block".to_owned(), Part::new(vec![block("body", 1.0)])),
            (
                "on".to_owned(),
                Part::new(vec![place("a", "block"), plane.into()]),
            ),
        ]));
        let engine = Engine::inline();
        let height = |ws: &Workspace| -> anyhow::Result<f64> {
            let built = build(&engine, ws, "on")?;
            let Some(Datum::Plane(plane)) = built.world.datums.get("above") else {
                anyhow::bail!("no plane");
            };
            assert!(
                plane
                    .normal()
                    .abs_diff_eq(workpiece_core::math::Point3::Z, 1e-9)
            );
            Ok(plane.origin().z)
        };
        assert!((height(&ws)? - 2.5).abs() < 1e-9);
        // The block is lifted: the plane goes with it.
        let lifted = Pose::from_translation(workpiece_core::math::Point3::new(0.0, 0.0, 10.0));
        let part = ws.part_mut("on")?.clone().placing("a", &lifted);
        ws.parts.insert("on".into(), part);
        assert!((height(&ws)? - 12.5).abs() < 1e-9);
        Ok(())
    }

    #[test]
    fn steps_after_a_joint_see_the_parts_where_it_holds_them() -> anyhow::Result<()> {
        use workpiece_core::Source;
        use workpiece_core::world::{Datum, JointKind};
        use workpiece_ops_datum::{AddPlane, AddPlaneArgs, PlaneMode};
        use workpiece_ops_instances::connector::Snap;
        use workpiece_ops_instances::{Anchor, Hold};
        // A block of 1 fastened, its bottom against the top of a block of
        // 2 (at z = 1), `offset` off that top's middle; then a plane on
        // its top.
        let face = |instance: &[&str], name: &str| Anchor {
            instance: instance.iter().map(|s| (*s).to_owned()).collect(),
            source: Source::Topology(format!("body/{name}")),
            snap: Snap::FaceCentre,
        };
        let assembly = |offset: [f64; 3]| -> AnyOperation {
            let mut hold = Hold::joint(
                JointKind::Fastened,
                face(&["plate"], "top"),
                face(&[], "bottom"),
            );
            hold.mates[0].flip_with = true;
            hold.mates[0].offset = offset;
            AddPart {
                id: "pin".into(),
                args: AddPartArgs {
                    part: "pin".into(),
                    hold,
                    ..AddPartArgs::default()
                },
            }
            .into()
        };
        let top = Source::placed(&["pin".to_owned()], Source::Topology("body/top".into()));
        let plane: AnyOperation = AddPlane {
            id: "on".into(),
            args: AddPlaneArgs {
                mode: PlaneMode::Offset,
                references: vec![top.into()],
                distance: 0.0,
                ..AddPlaneArgs::default()
            },
        }
        .into();
        let fixed: AnyOperation = AddPart {
            id: "plate".into(),
            args: AddPartArgs {
                part: "plate".into(),
                hold: Hold::fixed(),
                ..AddPartArgs::default()
            },
        }
        .into();
        let engine = Engine::inline();
        for offset in [[0.0; 3], [3.2, -1.0, 0.5]] {
            let ws = Workspace::new(BTreeMap::from([
                ("plate".to_owned(), Part::new(vec![block("body", 2.0)])),
                ("pin".to_owned(), Part::new(vec![block("body", 1.0)])),
                (
                    "all".to_owned(),
                    Part::new(vec![fixed.clone(), assembly(offset), plane.clone()]),
                ),
            ]));
            let built = build(&engine, &ws, "all")?;
            // The pin's origin is half its height over the plate's top,
            // moved by the offset in the top's own frame (x and y the
            // part's, z up).
            let [u, v, w] = offset;
            let at = workpiece_core::math::Point3::new(u, v, 1.5 + w);
            let pin = built.world.instances["pin"].pose.translation();
            assert!(pin.abs_diff_eq(at, 1e-7), "{pin} for {offset:?}");
            // The plane, a step after, is on its top where the joint
            // holds it: not where the pin was put (its top at 0.5).
            let Some(Datum::Plane(on)) = built.world.datums.get("on") else {
                anyhow::bail!("no plane");
            };
            assert!((on.origin().z - (2.0 + w)).abs() < 1e-7, "{}", on.origin());
        }
        Ok(())
    }

    #[test]
    fn parts_build_side_by_side_and_come_out_the_same() -> anyhow::Result<()> {
        let ws = workspace(24);
        let alone = build(&Engine::inline(), &ws, "all")?;
        let engine = Engine::threaded(4);
        let together = build(&engine, &ws, "all")?;
        assert_eq!(together.world.instances.len(), 48);
        for (name, instance) in &alone.world.instances {
            let other = &together.world.instances[name];
            assert_eq!(
                instance.world.topo.vertices().iter().count(),
                other.world.topo.vertices().iter().count()
            );
            assert_eq!(instance.world.stamp, other.world.stamp);
        }
        Ok(())
    }

    #[test]
    fn a_cancelled_build_stops() {
        let ws = workspace(2);
        let cancel = Cancel::new();
        cancel.cancel();
        let err = Engine::inline()
            .build(&Arc::new(ws), "all", None, &|_| true, &cancel)
            .expect_err("cancelled");
        assert!(err.downcast_ref::<Cancelled>().is_some(), "{err:#}");
    }

    #[test]
    fn a_step_file_is_a_part_of_its_own() -> anyhow::Result<()> {
        // A file that is no STEP file still stands for a part: building
        // it fails where the import does, not for there being no part.
        let ws = Workspace::default();
        assert!(ws.resolve("missing.wpc").is_err());
        let part = ws.resolve("bracket.STEP")?;
        assert!(matches!(&part.program[..], [op] if op.is::<workpiece_ops_step::ImportStep>()));
        let err = build(&Engine::inline(), &ws, "bracket.step").expect_err("no such file");
        assert!(format!("{err:#}").contains("bracket.step"), "{err:#}");
        Ok(())
    }
}

#[cfg(test)]
mod step_tests {
    use super::*;
    use std::collections::BTreeMap;
    use workpiece_ops_instances::{AddPart, AddPartArgs};

    /// A rod of radius and height in inches, as most systems write a
    /// cylinder.
    const ROD: &str = include_str!("../../../ops/step/src/rod.step");

    #[test]
    fn a_step_file_placed_twice_is_imported_once_and_stays_imported() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("workpiece-engine-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("rod.step").to_string_lossy().into_owned();
        std::fs::write(&file, ROD)?;
        let place = |id: &str, _: f64| -> crate::AnyOperation {
            AddPart {
                id: id.into(),
                args: AddPartArgs {
                    part: file.clone(),
                    variant: None,
                    hold: Default::default(),
                    bodies: Vec::new(),
                },
            }
            .into()
        };
        let mut ws = Workspace::new(BTreeMap::from([(
            "pair".to_owned(),
            Part::new(vec![place("left", -50.0), place("right", 50.0)]),
        )]));
        let engine = Engine::threaded(2);
        let build = |ws: &Workspace| {
            engine.build(
                &Arc::new(ws.clone()),
                "pair",
                None,
                &|_| true,
                &Cancel::new(),
            )
        };
        let first = build(&ws)?;
        let rod = &first.world.instances["left"].world;
        // The file's solid, as a body of the part that stands for it.
        assert!(rod.topo.lookup("step").is_some());
        assert!(Arc::ptr_eq(rod, &first.world.instances["right"].world));
        // The part that places it changes; the import does not run
        // again.
        ws.part_mut("pair")?.program.push(place("third", 0.0));
        let second = build(&ws)?;
        assert!(Arc::ptr_eq(rod, &second.world.instances["third"].world));
        // The file changes: it is read again.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&file, ROD.replace("'rod'", "'bar'"))?;
        let third = build(&ws)?;
        assert!(!Arc::ptr_eq(rod, &third.world.instances["left"].world));
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
