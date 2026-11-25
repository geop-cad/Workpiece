//! Combine: a boolean of two bodies.

use anyhow::Context;
use shapecraft_boolean::{Op, combine, read};
use shapecraft_core::world::topo::BodyId;
use shapecraft_core::{Builder, Operation};

use shapecraft_shape::bodies::Combine;
use shapecraft_shape::ui;

/// The two bodies and what to make of them.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct CombineArgs {
    /// The body that is changed.
    #[arg(label = "Body")]
    pub target: Option<shapecraft_core::Reference<shapecraft_core::world::topo::BodyId>>,
    /// How: joined, cut or intersected.
    #[arg(among = "WAYS", label = "Operation")]
    pub how: Combine,
    /// The body it is changed with; it is used up.
    #[arg(label = "With")]
    pub tool: Option<shapecraft_core::Reference<shapecraft_core::world::topo::BodyId>>,
}

impl Default for CombineArgs {
    fn default() -> Self {
        Self {
            target: None,
            tool: None,
            how: Combine::Join,
        }
    }
}

/// Joins a body to another, cuts it out of it, or keeps what they share.
/// The result has the first body's name; the second is gone.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CombineBodies {
    /// The operation's id.
    pub id: String,
    /// The bodies and what to make of them.
    pub args: CombineArgs,
}

/// The ways two bodies combine.
const WAYS: [Combine; 3] = [Combine::Join, Combine::Cut, Combine::Intersect];

impl Operation for CombineBodies {
    type Args = CombineArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &CombineArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut CombineArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = shapecraft_shape::bodies::Out::default();
        let topo = &builder.tracked().topo;
        let body = |source: &Option<ui::BodyRef>, what: &str| {
            let name = source
                .as_ref()
                .with_context(|| format!("pick {what}"))?
                .name();
            let id = topo
                .resolve::<BodyId>(name)
                .with_context(|| format!("there is no body {name:?}"))?;
            anyhow::Ok((id, name.to_owned()))
        };
        let (target, name) = body(&self.args.target, "the body to change")?;
        let (tool, _) = body(&self.args.tool, "the body to change it with")?;
        anyhow::ensure!(target != tool, "a body cannot be combined with itself");
        let op = match self.args.how {
            Combine::Cut => Op::Cut,
            Combine::Intersect => Op::Intersect,
            Combine::Join | Combine::NewBody => Op::Union,
        };
        let outcome = combine(op, &read(topo, target)?, &read(topo, tool)?)?;
        out.remove(topo, target);
        out.remove(topo, tool);
        if !outcome.solid.is_empty() {
            out.write(&outcome.solid, &name);
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let op = self;
        let name = |s: &Option<
            shapecraft_core::Reference<shapecraft_core::world::topo::BodyId>,
        >| { s.as_ref().map_or("(not picked)", |s| s.name()).to_owned() };
        format!(
            "{} {} with {}",
            op.args.how.label().to_lowercase(),
            name(&op.args.target),
            name(&op.args.tool)
        )
    }
}
