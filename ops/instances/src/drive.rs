//! The [`Drive`] operation: a joint held at a place along its travel.

use anyhow::Context;
use shapecraft_core::{Builder, EditContext, Measure, Operation};

/// The joint, and where it is held.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct DriveArgs {
    /// The joint: the name of the step that made it.
    #[arg(
        one_of = "driven(context).into_iter().map(|j| j.0).collect()",
        empty = "Choose a joint that turns or slides…"
    )]
    pub joint: Option<String>,
    /// Where it is held: an angle in degrees for a joint that turns, a
    /// distance for one that slides. (A parameter here, and the part's
    /// variants run the joint through its range.)
    #[arg(label_by = "if self.sliding(context) { \"Distance\" } else { \"Angle (°)\" }")]
    pub at: Measure,
    /// For a joint that both turns and slides: whether it is its
    /// sliding that is held.
    #[serde(default)]
    #[arg(
        label = "Held",
        options = "Its turning|Its sliding",
        when = "self.of(context) == (true, true)"
    )]
    pub slide: bool,
}

impl Default for DriveArgs {
    fn default() -> Self {
        Self {
            joint: None,
            at: Measure::new(0.0),
            slide: false,
        }
    }
}

/// Holds a joint of the assembly at a place along its travel: a hinge
/// at an angle, a slide at a distance. What else the joint's parts are
/// joined to follows.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Drive {
    /// The step's id.
    pub id: String,
    /// The joint, and where it is held.
    pub args: DriveArgs,
}

impl Drive {
    /// A drive called `id`, its joint still to choose.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            args: DriveArgs::default(),
        }
    }
}

impl Operation for Drive {
    type Args = DriveArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &DriveArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut DriveArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: what it makes is put in by the
        // builder.)
        let mut out = shapecraft_boolean::Out::default();
        let name = self
            .args
            .joint
            .as_ref()
            .context("choose the joint to drive")?;
        let at = (*builder).measure(&self.args.at, "where the joint is held")?;
        // (The joint, held where it is driven to.)
        let mut joint = builder
            .joint(name)
            .cloned()
            .with_context(|| format!("there is no joint {name}"))?;
        let (turns, slides) = (joint.kind.turns(), joint.kind.slides());
        anyhow::ensure!(
            turns || slides,
            "the joint {name} neither turns nor slides: there is nothing to drive"
        );
        let within = |limits: Option<(f64, f64)>, at: f64, shown: f64| {
            let inside = limits.is_none_or(|(lo, hi)| at >= lo - 1e-9 && at <= hi + 1e-9);
            anyhow::ensure!(inside, "{shown} is past the limits of the joint {name}");
            Ok(Some((at, at)))
        };
        if slides && (self.args.slide || !turns) {
            joint.limits.distance = within(joint.limits.distance, at, at)?;
        } else {
            joint.limits.angle = within(joint.limits.angle, at.to_radians(), at)?;
        }
        out.made.joints_changed.push(joint);
        Ok(out.made)
    }

    fn summary(&self) -> String {
        match &self.args.joint {
            Some(joint) => format!("{joint} at {}", self.args.at),
            None => "no joint yet".to_owned(),
        }
    }
}

/// The joints there are to drive: those that turn or slide, each with
/// whether it does.
fn driven(context: &EditContext<'_>) -> Vec<(String, bool, bool)> {
    let joints = context.world.joints.iter();
    joints
        .filter(|j| j.kind.turns() || j.kind.slides())
        .map(|j| (j.name.clone(), j.kind.turns(), j.kind.slides()))
        .collect()
}

impl DriveArgs {
    /// Whether the joint chosen turns, and whether it slides (one that
    /// turns, while none is chosen).
    fn of(&self, context: &EditContext<'_>) -> (bool, bool) {
        let chosen = driven(context)
            .into_iter()
            .find(|j| Some(&j.0) == self.joint.as_ref());
        chosen.map_or((true, false), |j| (j.1, j.2))
    }

    /// Whether it is the joint's sliding that is held.
    fn sliding(&self, context: &EditContext<'_>) -> bool {
        let (turns, slides) = self.of(context);
        slides && (self.slide || !turns)
    }
}
