//! Delete body: bodies taken out of the part.

use anyhow::Context;
use workpiece_core::world::topo::{BodyId, BodyKind};
use workpiece_core::{Builder, Operation, Reference};

/// The bodies that go.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    workpiece_core::Arguments,
)]
pub struct DeleteBodyArgs {
    /// The bodies deleted: solids, surfaces or meshes.
    #[arg(label = "Bodies", required)]
    pub bodies: Vec<Reference<BodyId>>,
}

/// Takes bodies out of the part: what later steps do, they do without
/// them. (The steps that made them stay; a tool body used up, a half
/// left over by a split, a surface that was only there to cut by.)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeleteBody {
    /// The operation's id.
    pub id: String,
    /// The bodies that go.
    pub args: DeleteBodyArgs,
}

impl Operation for DeleteBody {
    type Args = DeleteBodyArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &DeleteBodyArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut DeleteBodyArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        anyhow::ensure!(!self.args.bodies.is_empty(), "pick the bodies to delete");
        let world = builder.tracked();
        let mut made = workpiece_core::Made::default();
        for body in &self.args.bodies {
            let id = world.resolve(body)?;
            let kind = world.topo.body(id).map(|b| b.kind);
            anyhow::ensure!(kind.is_some(), "there is no body {:?}", body.name());
            let _: Option<BodyKind> = kind;
            let name = world.topo.name_of(id).context("a body without a name")?;
            made.removed.push(name.to_owned());
        }
        Ok(made)
    }

    fn summary(&self) -> String {
        let names: Vec<&str> = self.args.bodies.iter().map(|b| b.name()).collect();
        match names.len() {
            0 => "nothing yet".to_owned(),
            1..=3 => names.join(", "),
            n => format!("{n} bodies"),
        }
    }
}
