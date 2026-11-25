//! [`AddTorus`]: a ring around a frame's z axis.

use anyhow::Context;
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::DatumFrame;
use workpiece_core::world::topo::{NurbsSurface, Orientation};
use workpiece_core::{Builder, Measure, Operation, Reference};

use crate::brep::{SolidBuilder, circle, circle_knots, unit_circle};

/// Where the torus goes and how big it is.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct AddTorusArgs {
    /// The frame at its centre; it goes around the frame's z axis.
    pub frame: Option<Reference<DatumFrame>>,
    /// The radius of the circle its tube runs along.
    pub radius: Measure,
    /// The radius of its tube.
    #[arg(label = "Tube radius", row)]
    pub tube: Measure,
}

impl Default for AddTorusArgs {
    fn default() -> Self {
        Self {
            frame: None,
            radius: Measure::new(1.0),
            tube: Measure::new(0.25),
        }
    }
}

/// Adds a solid torus, named after the operation's id: its one face
/// `{id}/face`, the circle around its outside `{id}/equator`, the
/// circle around its tube `{id}/seam`, and where the two meet
/// `{id}/start`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AddTorus {
    /// The operation's id; also the prefix of the torus's names.
    pub id: String,
    /// The frame and the two radii.
    pub args: AddTorusArgs,
}

impl Operation for AddTorus {
    type Args = AddTorusArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &AddTorusArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut AddTorusArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let major = (*builder).measure(&self.args.radius, "the radius")?;
        let minor = (*builder).measure(&self.args.tube, "the tube's radius")?;
        anyhow::ensure!(
            minor.is_finite() && minor > 0.0,
            "the tube's radius {minor} is not positive"
        );
        anyhow::ensure!(
            major.is_finite() && major > minor,
            "the tube (radius {minor}) is wider than the ring it runs along (radius {major})"
        );
        let frame = self.args.frame.as_ref().context("pick a frame")?;
        // (It reads its frame, and no more; what it makes of that is
        // written by the builder.)
        let pose = builder.resolve(frame)?.pose();
        let shape = move |topo: &mut workpiece_core::world::topo::Topo, name: &str| {
            let mut solid = SolidBuilder::new(topo, name, pose)?;
            let (x, y, z) = (
                solid.turn(Point3::X),
                solid.turn(Point3::Y),
                solid.turn(Point3::Z),
            );
            // u runs around the axis, v around the tube (from its outside,
            // upwards): ∂u × ∂v points out of the tube.
            let (mut points, mut weights) = (Vec::new(), Vec::new());
            for (around, wu) in unit_circle() {
                for (across, wv) in unit_circle() {
                    let out = major + minor * across.x;
                    points.push(solid.place(Point3::new(
                        around.x * out,
                        around.y * out,
                        minor * across.y,
                    )));
                    weights.push(wu * wv);
                }
            }
            let surface = NurbsSurface::new(
                2,
                2,
                circle_knots(),
                circle_knots(),
                9,
                9,
                points,
                Some(weights),
            )?;
            let start = solid.vertex(solid.place(Point3::new(major + minor, 0.0, 0.0)), "start")?;
            let equator = circle(solid.place(Point3::ZERO), x, y, major + minor)?;
            let equator = solid.edge(equator, start, start, "equator")?;
            let seam = circle(solid.place(Point3::new(major, 0.0, 0.0)), x, z, minor)?;
            let seam = solid.edge(seam, start, start, "seam")?;
            use Orientation::{Forward, Reversed};
            let uv = |u: f64, v: f64| Point2::new(u, v);
            let around = vec![
                solid.coedge(equator, Forward, uv(0.0, 0.0), uv(1.0, 0.0))?,
                solid.coedge(seam, Forward, uv(1.0, 0.0), uv(1.0, 1.0))?,
                solid.coedge(equator, Reversed, uv(0.0, 1.0), uv(1.0, 1.0))?,
                solid.coedge(seam, Reversed, uv(0.0, 0.0), uv(0.0, 1.0))?,
            ];
            let face = solid.face(surface, around, "face")?;
            solid.solid(vec![face])?;
            Ok(())
        };
        Ok(workpiece_core::Made::body(&self.id, shape))
    }

    fn summary(&self) -> String {
        format!("radius {}, tube {}", self.args.radius, self.args.tube)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workpiece_core::Standalone;
    use workpiece_core::world::ORIGIN;

    #[test]
    fn tori_are_valid_solids_of_one_face() -> anyhow::Result<()> {
        let mut b = Standalone::new();
        AddTorus {
            id: "ring".into(),
            args: AddTorusArgs {
                frame: Some(Reference::datum(ORIGIN)),
                ..AddTorusArgs::default()
            },
        }
        .apply(&mut b)?;
        let topo = &b.world.topo;
        assert_eq!(topo.validate(), Ok(()));
        assert_eq!((topo.faces().len(), topo.edges().len()), (1, 2));
        Ok(())
    }
}
