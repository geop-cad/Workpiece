//! [`AddCone`]: a cone, or one cut off, standing on a frame.

use shapecraft_core::world::DatumFrame;
use shapecraft_core::{Builder, Measure, Operation, Reference};

/// Where the cone goes and how big it is.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct AddConeArgs {
    /// The frame at the centre of the base; the cone rises along its z
    /// axis.
    pub frame: Option<Reference<DatumFrame>>,
    /// The radius of the base.
    pub radius: Measure,
    /// The radius at the top: nothing, for a cone that comes to a point.
    #[arg(label = "Top radius", row)]
    pub top: Measure,
    /// The height along the frame's z axis.
    pub height: Measure,
}

impl Default for AddConeArgs {
    fn default() -> Self {
        Self {
            frame: None,
            radius: Measure::new(0.5),
            top: Measure::new(0.0),
            height: Measure::new(1.0),
        }
    }
}

/// Adds a solid cone, named after the operation's id as a cylinder's
/// parts are: `{id}/side`, `{id}/bottom`, and — cut off — `{id}/top`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AddCone {
    /// The operation's id; also the prefix of the cone's names.
    pub id: String,
    /// The frame, the radii and the height.
    pub args: AddConeArgs,
}

impl Operation for AddCone {
    type Args = AddConeArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &AddConeArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut AddConeArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        let r = (*builder).measure(&self.args.radius, "the radius")?;
        let top = (*builder).measure(&self.args.top, "the radius at the top")?;
        let h = (*builder).measure(&self.args.height, "the height")?;
        crate::add_cylinder::frustum(builder, &self.id, &self.args.frame, r, top, h)
    }

    fn summary(&self) -> String {
        let a = &self.args;
        format!("{} to {} over {}", a.radius, a.top, a.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shapecraft_core::Standalone;
    use shapecraft_core::world::ORIGIN;

    #[test]
    fn cones_come_to_a_point_or_are_cut_off() -> anyhow::Result<()> {
        for (top, faces, edges) in [(0.0, 2, 3), (0.25, 3, 5)] {
            let mut b = Standalone::new();
            AddCone {
                id: "cone".into(),
                args: AddConeArgs {
                    frame: Some(Reference::datum(ORIGIN)),
                    top: Measure::new(top),
                    ..AddConeArgs::default()
                },
            }
            .apply(&mut b)?;
            let topo = &b.world.topo;
            assert_eq!(topo.validate(), Ok(()));
            assert_eq!((topo.faces().len(), topo.edges().len()), (faces, edges));
        }
        Ok(())
    }
}
