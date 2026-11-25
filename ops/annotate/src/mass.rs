//! Mass properties: the step that asks how bodies weigh and turn —
//! their mass, their centre of gravity, their moments of inertia. The
//! view shows them as the ellipsoid that weighs and turns alike.

use anyhow::Context;
use shapecraft_boolean::{read, spread};
use shapecraft_core::world::topo::{BodyId, BodyKind};
use shapecraft_core::world::{MassProperties as Asked, Spread};
use shapecraft_core::{Builder, Made, Operation, Reference};

/// Which bodies are asked about.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    shapecraft_core::Arguments,
)]
pub struct MassPropertiesArgs {
    /// The bodies, taken together; none for every solid body.
    #[arg(label = "Bodies", empty = "All bodies")]
    pub bodies: Vec<Reference<BodyId>>,
}

/// Asks how bodies weigh and turn. Nothing of the model follows from
/// it: what it finds is read in the view, each body weighed by what it
/// is made of.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MassProperties {
    /// The operation's id: what the properties are called.
    pub id: String,
    /// Which bodies.
    pub args: MassPropertiesArgs,
}

impl Operation for MassProperties {
    type Args = MassPropertiesArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &MassPropertiesArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut MassPropertiesArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<Made<'_>> {
        let world = builder.tracked();
        let topo = &world.topo;
        let bodies: Vec<BodyId> = if self.args.bodies.is_empty() {
            topo.bodies()
                .iter()
                .filter(|(_, body)| body.kind == BodyKind::Solid)
                .map(|(id, _)| id)
                .collect()
        } else {
            let mut picked = Vec::new();
            for body in &self.args.bodies {
                picked.push(world.resolve(body)?);
            }
            picked
        };
        anyhow::ensure!(!bodies.is_empty(), "there is no solid body to weigh");
        let mut asked = Asked::default();
        for body in bodies {
            let name = topo.name_of(body).context("a body without a name")?;
            let solid = read(topo, body).with_context(|| format!("{name} is not a solid"))?;
            let of = spread(&solid);
            asked.bodies.push(Spread {
                body: name.to_owned(),
                volume: of.volume,
                centre: of.centre,
                inertia: of.inertia,
            });
        }
        Ok(Made {
            masses: vec![(self.id.clone(), asked)],
            ..Made::default()
        })
    }

    fn summary(&self) -> String {
        match &self.args.bodies[..] {
            [] => "of every body".to_owned(),
            [one] => format!("of {}", one.name()),
            many => format!("of {} bodies", many.len()),
        }
    }
}
