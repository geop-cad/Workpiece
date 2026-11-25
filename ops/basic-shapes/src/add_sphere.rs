//! [`AddSphere`]: a sphere centred on a frame.

use anyhow::Context;
use shapecraft_core::handles::LinearHandle;
use shapecraft_core::math::{Axis, Point2, Point3};
use shapecraft_core::widgets::VBox;
use shapecraft_core::world::DatumFrame;
use shapecraft_core::world::topo::{Curve3, NurbsSurface, Orientation};
use shapecraft_core::{Builder, EditVisualization, Measure, Operation, Reference, World};

use crate::brep::{SolidBuilder, circle_knots, unit_circle};
use crate::frame_handles;

/// Where the sphere goes and how big it is.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct AddSphereArgs {
    /// The frame the sphere is centred on; its z axis runs through the
    /// poles.
    pub frame: Option<Reference<DatumFrame>>,
    /// The radius.
    pub radius: Measure,
}

impl Default for AddSphereArgs {
    fn default() -> Self {
        Self {
            frame: None,
            radius: Measure::new(0.5),
        }
    }
}

/// Adds a solid sphere, named after the operation's id: the face
/// `{id}/face`, the seam `{id}/seam` (the meridian in the frame's xz
/// plane, towards +x) and the poles `{id}/south` and `{id}/north`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AddSphere {
    /// The operation's id; also the prefix of the sphere's names.
    pub id: String,
    /// The frame and the radius.
    pub args: AddSphereArgs,
}

/// The knots of a half circle of two quadratic arcs.
const HALF_CIRCLE_KNOTS: [f64; 8] = [0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0];

/// The half circle from the south to the north pole through +x, as
/// (distance from the axis, height) control points of a unit circle, with
/// weights.
fn meridian() -> [(Point2, f64); 5] {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    [
        (Point2::new(0.0, -1.0), 1.0),
        (Point2::new(1.0, -1.0), w),
        (Point2::new(1.0, 0.0), 1.0),
        (Point2::new(1.0, 1.0), w),
        (Point2::new(0.0, 1.0), 1.0),
    ]
}

impl Operation for AddSphere {
    type Args = AddSphereArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &AddSphereArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut AddSphereArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        let r = (*builder).measure(&self.args.radius, "the radius")?;
        anyhow::ensure!(r.is_finite() && r > 0.0, "radius {r} is not positive");
        let frame = self.args.frame.as_ref().context("pick a frame")?;
        // (It reads its frame, and no more; what it makes of that is
        // written by the builder.)
        let pose = builder.resolve(frame)?.pose();
        let shape = move |topo: &mut shapecraft_core::world::topo::Topo, name: &str| {
            let mut solid = SolidBuilder::new(topo, name, pose)?;

            // The surface of revolution of the meridian: u around the z axis,
            // v from the south to the north pole, so ∂u × ∂v points outwards.
            let (mut points, mut weights) = (Vec::new(), Vec::new());
            for (c, wc) in unit_circle() {
                for (m, wm) in meridian() {
                    points.push(solid.place(Point3::new(c.x * m.x, c.y * m.x, m.y) * r));
                    weights.push(wc * wm);
                }
            }
            let surface = NurbsSurface::new(
                2,
                2,
                circle_knots(),
                HALF_CIRCLE_KNOTS.to_vec(),
                9,
                5,
                points,
                Some(weights),
            )?;

            let (south, north) = (Point3::new(0.0, 0.0, -r), Point3::new(0.0, 0.0, r));
            let south = solid.vertex(solid.place(south), "south")?;
            let north = solid.vertex(solid.place(north), "north")?;
            let (seam_points, seam_weights) = meridian()
                .into_iter()
                .map(|(m, w)| (solid.place(Point3::new(m.x, 0.0, m.y) * r), w))
                .unzip();
            let seam = Curve3::new(
                2,
                HALF_CIRCLE_KNOTS.to_vec(),
                seam_points,
                Some(seam_weights),
            )?;
            let seam = solid.edge(seam, south, north, "seam")?;

            // Counter-clockwise around the (u, v) square: the south pole, the
            // seam up at u = 1, the north pole, the seam down at u = 0.
            let uv = |u: f64, v: f64| Point2::new(u, v);
            let coedges = vec![
                solid.pole(south, uv(0.0, 0.0), uv(1.0, 0.0))?,
                solid.coedge(seam, Orientation::Forward, uv(1.0, 0.0), uv(1.0, 1.0))?,
                solid.pole(north, uv(1.0, 1.0), uv(0.0, 1.0))?,
                solid.coedge(seam, Orientation::Reversed, uv(0.0, 0.0), uv(0.0, 1.0))?,
            ];
            let face = solid.face(surface, coedges, "face")?;
            solid.solid(vec![face])?;
            Ok(())
        };
        Ok(shapecraft_core::Made::body(&self.id, shape))
    }

    fn visualize_edit(
        &self,
        context: &shapecraft_core::EditContext<'_>,
        (): &(),
    ) -> EditVisualization<shapecraft_core::Edit<AddSphereArgs, ()>> {
        let world: &World = context.world;
        let AddSphereArgs { frame, radius } = self.args.clone();
        let with_radius = {
            let frame = frame.clone();
            move |radius: Measure| AddSphereArgs {
                frame: frame.clone(),
                radius,
            }
        };
        // The fields the arguments give; the handles are its own.
        let dialog = VBox::new(shapecraft_core::Arguments::fields(&self.args, context));
        // What a formula gives cannot be dragged.
        let handles = frame_handles(world, &frame, &[Axis::X], |_, origin, direction| {
            let with_radius = with_radius.clone();
            let drag = move |r: f64| with_radius(Measure::new(r));
            LinearHandle::new(origin, direction, radius.value, drag).filter(|_| radius.is_plain())
        });
        EditVisualization::new(dialog, handles).map(shapecraft_core::Edit::args)
    }

    fn summary(&self) -> String {
        let op = self;
        format!(
            "r {} on {}",
            op.args.radius,
            shapecraft_core::reference::described(&op.args.frame)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shapecraft_core::Standalone;
    use shapecraft_core::world::ORIGIN;
    use shapecraft_core::world::topo::FaceId;

    #[test]
    fn spheres_are_valid_named_solids() -> anyhow::Result<()> {
        let mut b = Standalone::new();
        let op = AddSphere {
            id: "ball".into(),
            args: AddSphereArgs {
                radius: 2.0.into(),
                frame: Some(Reference::datum(ORIGIN)),
            },
        };
        op.apply(&mut b)?;
        let topo = &b.world.topo;
        assert_eq!(topo.validate(), Ok(()));
        let face = topo.resolve::<FaceId>("ball/face").expect("named");
        let surface = topo
            .surface(topo.face(face).expect("face").surface)
            .expect("surface");
        // Every point is at the radius.
        for (u, v) in [(0.1, 0.2), (0.4, 0.5), (0.77, 0.9)] {
            assert!((surface.point_at(u, v).length() - 2.0).abs() < 1e-12);
        }
        Ok(())
    }
}
