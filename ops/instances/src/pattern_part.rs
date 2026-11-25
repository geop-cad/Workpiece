//! [`PatternPart`]: a placed part repeated, each a step from the one
//! before.

use anyhow::Context;
use shapecraft_core::world::{Connector, Joint, JointKind, Limits};
use shapecraft_core::{Builder, Operation, Pose};

/// The part repeated, how often, and the step between two.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct PatternPartArgs {
    /// The instance that is repeated: a part placed in this one.
    #[arg(
        one_of = "context.world.instances.keys().cloned().collect()",
        label = "Part",
        when = "!context.world.instances.is_empty()"
    )]
    pub of: Option<String>,
    /// How many there are in the end, the instance itself counted.
    pub count: usize,
    /// Where each is, seen from the one before: a move along a line, a
    /// turn about an axis, or both.
    #[arg(with = "step_field")]
    pub step: Pose,
}

impl Default for PatternPartArgs {
    fn default() -> Self {
        Self {
            of: None,
            count: 3,
            step: Pose::from_translation(shapecraft_core::math::Point3::X),
        }
    }
}

/// Repeats a placed part: the copies are instances `{id}/2`, `{id}/3`,
/// …, each fastened to the first, so that they go where it goes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PatternPart {
    /// The operation's id; also the prefix of the copies' names.
    pub id: String,
    /// What is repeated and how.
    pub args: PatternPartArgs,
}

impl Operation for PatternPart {
    type Args = PatternPartArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &PatternPartArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut PatternPartArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: what it makes is put in by the
        // builder.)
        let mut out = shapecraft_boolean::Out::default();
        let args = &self.args;
        let of = args.of.as_ref().context("pick the part to repeat")?;
        anyhow::ensure!(
            (2..=1000).contains(&args.count),
            "a pattern is of two or more"
        );
        let first = builder
            .placed(of)
            .with_context(|| format!("there is no placed part {of:?}"))?
            .clone();
        // Each a step further, seen from the first.
        let mut from_first = Pose::IDENTITY;
        for k in 2..=args.count {
            from_first = from_first * args.step;
            let name = format!("{}/{k}", self.id);
            anyhow::ensure!(
                !builder.peek().instances.contains_key(&name),
                "instance {name:?} exists"
            );
            let mut copy = first.clone();
            copy.pose = first.pose * from_first;
            copy.fixed = false;
            out.made.instances.push((name.clone(), Some(copy)));
            // Fastened to the first: it goes where that goes.
            out.made.joints.push(Joint {
                name: name.clone(),
                kind: JointKind::Fastened,
                a: Connector {
                    instance: Some(of.clone()),
                    frame: from_first,
                },
                b: Connector {
                    instance: Some(name),
                    frame: Pose::IDENTITY,
                },
                limits: Limits::default(),
            });
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let of = self.args.of.as_deref().unwrap_or("nothing yet");
        format!("{} of {of}", self.args.count)
    }
}

/// The field for the step from each copy to the next.
fn step_field(
    args: &PatternPartArgs,
    _: &shapecraft_core::EditContext<'_>,
) -> Vec<shapecraft_core::Widget<PatternPartArgs>> {
    let with = args.clone();
    let field = shapecraft_core::widgets::PoseInput::new(
        "Each from the one before",
        args.step,
        move |step| PatternPartArgs {
            step,
            ..with.clone()
        },
    );
    vec![field.into()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use shapecraft_core::math::Point3;
    use shapecraft_core::world::Instance;
    use shapecraft_core::{Standalone, World};
    use std::sync::Arc;

    #[test]
    fn placed_parts_are_repeated_and_fastened_to_the_first() -> anyhow::Result<()> {
        let mut b = Standalone::new();
        b.world.instances.insert(
            "bolt".into(),
            Instance {
                pose: Pose::from_translation(Point3::new(0.0, 5.0, 0.0)),
                world: Arc::new(World::new()),
                fixed: true,
            },
        );
        PatternPart {
            id: "row".into(),
            args: PatternPartArgs {
                of: Some("bolt".into()),
                count: 4,
                step: Pose::from_translation(Point3::new(2.0, 0.0, 0.0)),
            },
        }
        .apply(&mut b)?;
        assert_eq!(b.world.instances.len(), 4);
        let last = &b.world.instances["row/4"];
        let at = last.pose.transform_point(Point3::ZERO);
        assert!(at.distance(Point3::new(6.0, 5.0, 0.0)) < 1e-12, "{at}");
        assert_eq!(b.world.joints.len(), 3);
        assert!(b.world.joints.iter().all(|j| j.kind == JointKind::Fastened));
        Ok(())
    }
}
