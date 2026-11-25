//! What the steps of a feature did to the bodies, and how they are done
//! again somewhere else: the common ground of the feature pattern and
//! the feature mirror.

use std::sync::Arc;

use anyhow::Context;
use workpiece_boolean::{Op, Solid, combine, read};
use workpiece_core::math::Point3;
use workpiece_core::world::topo::{BodyKind, FaceId, Topo};
use workpiece_core::{Builder, Reference, World};

/// The step that made what is called `name`: what its name starts with.
fn step_of(name: &str) -> &str {
    name.split('/').next().unwrap_or(name)
}

/// The steps that made the picked faces, in order of name, each once.
pub(crate) fn steps_of(faces: &[Reference<FaceId>]) -> Vec<String> {
    let mut steps: Vec<String> = faces
        .iter()
        .map(|face| step_of(face.name()).to_owned())
        .collect();
    steps.sort();
    steps.dedup();
    steps
}

/// The solid bodies of `topo` by name.
fn solids(topo: &Topo) -> Vec<(String, Solid)> {
    topo.bodies()
        .iter()
        .filter(|(_, body)| body.kind == BodyKind::Solid)
        .filter_map(|(id, _)| Some((topo.name_of(id)?.to_owned(), read(topo, id).ok()?)))
        .collect()
}

/// `solid` with every name `to` gives another for changed to that.
fn renamed(mut solid: Solid, to: &dyn Fn(&str) -> Option<String>) -> Solid {
    let one = |name: &mut Option<String>| {
        if let Some(new) = name.as_deref().and_then(to) {
            *name = Some(new);
        }
    };
    let many = |names: &mut Vec<String>| {
        for name in names.iter_mut() {
            if let Some(new) = to(name) {
                *name = new;
            }
        }
    };
    solid.vertices.iter_mut().for_each(|v| one(&mut v.name));
    for edge in &mut solid.edges {
        one(&mut edge.name);
        many(&mut edge.also);
    }
    for face in &mut solid.faces {
        one(&mut face.name);
        many(&mut face.also);
    }
    solid
}

/// What marks, in a body the steps are done on again, the names of
/// what they made before: set aside while they make the same anew.
const ASIDE: &str = "\u{1}/";

/// How a body is taken from where the steps are done again, and back.
pub(crate) struct Move<'a> {
    /// From the world to where the steps are done again.
    pub(crate) back: &'a dyn Fn(Point3) -> Point3,
    /// And the outcome back into the world.
    pub(crate) there: &'a dyn Fn(Point3) -> Point3,
    /// Whether the two are reflections.
    pub(crate) mirrored: bool,
}

/// What the steps did to one body.
pub(crate) struct Change {
    /// The body's name.
    pub(crate) name: String,
    /// Whether the body was there before the steps (else they made it).
    pub(crate) existed: bool,
    /// What they added to it (all of it, of a body they made).
    pub(crate) added: Solid,
    /// What they took from it.
    pub(crate) taken: Solid,
}

/// What a set of steps did, and the world before them.
pub(crate) struct Features {
    /// The steps, in order of name.
    pub(crate) steps: Vec<String>,
    /// The world before the first of them.
    pub(crate) before: Arc<World>,
    /// What they did to each body they changed.
    pub(crate) changes: Vec<Change>,
}

impl Features {
    /// What the steps of the picked faces did to the bodies.
    pub(crate) fn of(builder: &dyn Builder, faces: &[Reference<FaceId>]) -> anyhow::Result<Self> {
        let steps = steps_of(faces);
        let (before_world, after) = builder.spanning(&steps)?;
        let (before, after) = (solids(&before_world.topo), solids(&after.topo));
        let mut changes = Vec::new();
        for (name, now) in after {
            let was = before.iter().find(|(n, _)| *n == name).map(|(_, s)| s);
            let (added, taken) = match was {
                Some(was) => (
                    combine(Op::Cut, &now, was)
                        .with_context(|| format!("what was added to {name}"))?
                        .solid,
                    combine(Op::Cut, was, &now)
                        .with_context(|| format!("what was taken from {name}"))?
                        .solid,
                ),
                None => (now, Solid::default()),
            };
            if added.is_empty() && taken.is_empty() {
                continue;
            }
            changes.push(Change {
                name,
                existed: was.is_some(),
                added,
                taken,
            });
        }
        Ok(Self {
            steps,
            before: before_world,
            changes,
        })
    }

    /// Whether `name` is of one of the steps.
    fn of_steps(&self, name: &str) -> bool {
        self.steps.iter().any(|s| {
            name == s
                || name
                    .strip_prefix(s.as_str())
                    .is_some_and(|r| r.starts_with('/'))
        })
    }

    /// The body `name`, as it is in `result`, with the steps done on it
    /// again where `by` takes it: the body taken by `by.back`, the steps
    /// done on it in the world as it was before them, and what comes
    /// of it taken by `by.there`. What they make is named under `under`. None where this adds and takes nothing new (a
    /// step that finds its place by the body itself does on the body
    /// taken back just what it did before: that is not a copy).
    ///
    /// The steps are done in the world they were done in, so their
    /// sketch planes, directions and senses are what they were: only
    /// the body is taken to and fro.
    pub(crate) fn redone(
        &self,
        builder: &dyn Builder,
        name: &str,
        result: &Solid,
        by: &Move<'_>,
        under: &str,
    ) -> anyhow::Result<Option<Solid>> {
        // The body where the steps find it as the copy would: what
        // they made of it before under other names for now.
        let aside = renamed(result.mapped(by.back, by.mirrored, 1.0)?, &|n| {
            self.of_steps(n).then(|| format!("{ASIDE}{n}"))
        });
        let mut on = (*self.before).clone();
        if let Some(old) = crate::arrange::named_solid(&on.topo, name) {
            workpiece_boolean::remove(&mut on.topo, old);
        }
        workpiece_boolean::write(&mut on.topo, &aside, name)?;
        let after = builder.redone(&self.steps, on)?;
        let body = crate::arrange::named_solid(&after.topo, name)
            .with_context(|| format!("{name} is gone"))?;
        let made = renamed(read(&after.topo, body)?, &|n| {
            if n.contains(ASIDE) {
                Some(n.replace(ASIDE, ""))
            } else {
                self.of_steps(n).then(|| format!("{under}/{n}"))
            }
        });
        let (was, now) = (
            workpiece_boolean::measure(result),
            workpiece_boolean::measure(&made),
        );
        let same = |a: f64, b: f64| (a - b).abs() <= 1e-9 * (a.abs() + b.abs());
        if same(was.volume, now.volume) && same(was.area, now.area) {
            return Ok(None);
        }
        Ok(Some(made.mapped(by.there, by.mirrored, 1.0)?))
    }
}

/// The callouts of the holes among the steps that made `faces`, as of
/// `times` as many holes (each hole repeated so many times over): what
/// a pattern or a mirror image of them says of the model, in their
/// place.
pub(crate) fn callouts_times(
    builder: &dyn workpiece_core::Builder,
    faces: &[workpiece_core::Reference<workpiece_core::world::topo::FaceId>],
    times: usize,
) -> Vec<(String, workpiece_core::world::Annotation)> {
    let annotations = &builder.tracked().annotations;
    steps_of(faces)
        .into_iter()
        .filter_map(|step| {
            let key = format!("{step}/callout");
            let said = annotations.get(&key)?.counted(times)?;
            Some((key, said))
        })
        .collect()
}
