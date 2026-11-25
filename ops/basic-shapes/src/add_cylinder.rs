//! [`AddCylinder`]: a cylinder standing on a frame.

use anyhow::Context;
use workpiece_core::handles::LinearHandle;
use workpiece_core::math::{Axis, Point2, Point3};
use workpiece_core::widgets::VBox;
use workpiece_core::world::DatumFrame;
use workpiece_core::world::topo::{Curve3, NurbsSurface, Orientation};
use workpiece_core::{Builder, EditVisualization, Measure, Operation, Reference, World};

use crate::brep::{SolidBuilder, circle, circle_knots, unit_circle};
use crate::frame_handles;

/// Where the cylinder goes and how big it is.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct AddCylinderArgs {
    /// The frame at the centre of the bottom; the cylinder rises along
    /// its z axis.
    pub frame: Option<Reference<DatumFrame>>,
    /// The radius.
    pub radius: Measure,
    /// The height along the frame's z axis.
    #[arg(row)]
    pub height: Measure,
}

impl Default for AddCylinderArgs {
    fn default() -> Self {
        Self {
            frame: None,
            radius: Measure::new(0.5),
            height: Measure::new(1.0),
        }
    }
}

/// Adds a solid cylinder, named after the operation's id: faces
/// `{id}/side`, `{id}/bottom` and `{id}/top`; edges `{id}/bottom-rim`,
/// `{id}/top-rim`, the side's seam `{id}/seam` and the caps' seams
/// `{id}/bottom-seam` and `{id}/top-seam`; vertices `{id}/bottom-start`,
/// `{id}/top-start` (where the rims start, towards +x) and the caps'
/// centres `{id}/bottom-centre` and `{id}/top-centre`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AddCylinder {
    /// The operation's id; also the prefix of the cylinder's names.
    pub id: String,
    /// The frame, the radius and the height.
    pub args: AddCylinderArgs,
}

impl Operation for AddCylinder {
    type Args = AddCylinderArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &AddCylinderArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut AddCylinderArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let r = (*builder).measure(&self.args.radius, "the radius")?;
        let h = (*builder).measure(&self.args.height, "the height")?;
        frustum(builder, &self.id, &self.args.frame, r, r, h)
    }

    fn visualize_edit(
        &self,
        context: &workpiece_core::EditContext<'_>,
        (): &(),
    ) -> EditVisualization<workpiece_core::Edit<AddCylinderArgs, ()>> {
        let world: &World = context.world;
        let args = self.args.clone();
        let with_radius = {
            let args = args.clone();
            move |radius: Measure| AddCylinderArgs {
                radius,
                ..args.clone()
            }
        };
        let with_height = {
            let args = args.clone();
            move |height: Measure| AddCylinderArgs {
                height,
                ..args.clone()
            }
        };
        // The fields the arguments give; the handles are its own.
        let dialog = VBox::new(workpiece_core::Arguments::fields(&self.args, context));
        // What a formula gives cannot be dragged.
        let handles = frame_handles(
            world,
            &args.frame,
            &[Axis::X, Axis::Z],
            |axis, origin, direction| {
                let radial = axis == Axis::X;
                let measure = if radial { &args.radius } else { &args.height };
                let (with_radius, with_height) = (with_radius.clone(), with_height.clone());
                let drag = move |x: f64| {
                    let x = Measure::new(x);
                    if radial {
                        with_radius(x)
                    } else {
                        with_height(x)
                    }
                };
                LinearHandle::new(origin, direction, measure.value, drag)
                    .filter(|_| measure.is_plain())
            },
        );
        EditVisualization::new(dialog, handles).map(workpiece_core::Edit::args)
    }

    fn summary(&self) -> String {
        let op = self;
        format!(
            "r {} h {} on {}",
            op.args.radius,
            op.args.height,
            workpiece_core::reference::described(&op.args.frame)
        )
    }
}

/// Adds the solid between the disk of radius `r` on `frame` and the
/// one of radius `top` a height `h` up its z axis — a cylinder, a cone
/// cut off, or (with no radius at the top) a cone — called `id`.
pub(crate) fn frustum<'a>(
    builder: &dyn Builder,
    id: &'a str,
    frame: &Option<Reference<DatumFrame>>,
    r: f64,
    top: f64,
    h: f64,
) -> anyhow::Result<workpiece_core::Made<'a>> {
    anyhow::ensure!(r.is_finite() && r >= 0.0, "radius {r} is less than nothing");
    anyhow::ensure!(
        top.is_finite() && top >= 0.0,
        "radius {top} is less than nothing"
    );
    anyhow::ensure!(
        r > 0.0 || top > 0.0,
        "a cone has a radius at one end at least"
    );
    anyhow::ensure!(h.is_finite() && h > 0.0, "height {h} is not positive");
    let frame = frame.as_ref().context("pick a frame")?;
    // (It reads its frame, and no more; what it makes of that is
    // written by the builder.)
    let pose = builder.resolve(frame)?.pose();
    // A cone that stands on its point is the cone that comes to a point
    // at its top, turned over: made from the frame at its top, upside
    // down. (Its names are then those of the turned cone: its `bottom`
    // is the flat face, uppermost.)
    let (pose, r, top) = if r > 0.0 {
        (pose, r, top)
    } else {
        let over = workpiece_core::Pose::from_axes(Point3::new(0.0, 0.0, h), Point3::X, -Point3::Y);
        (pose * over, top, 0.0)
    };
    let shape = move |topo: &mut workpiece_core::world::topo::Topo, name: &str| {
        let mut solid = SolidBuilder::new(topo, name, pose)?;
        let (x, y) = (solid.turn(Point3::X), solid.turn(Point3::Y));
        let at = |z: f64| Point3::new(0.0, 0.0, z);

        // A surface whose u runs around the circle (counter-clockwise about
        // z, as the rims do) and whose v runs through the rows of rings:
        // `rows` gives each row's radius and height.
        let ring_surface = |solid: &SolidBuilder<'_>, rows: [(f64, f64); 2]| {
            let (mut points, mut weights) = (Vec::new(), Vec::new());
            for (c, w) in unit_circle() {
                for (radius, z) in rows {
                    points.push(solid.place(Point3::new(c.x * radius, c.y * radius, z)));
                    weights.push(w);
                }
            }
            NurbsSurface::new(
                2,
                1,
                circle_knots(),
                vec![0.0, 0.0, 1.0, 1.0],
                9,
                2,
                points,
                Some(weights),
            )
        };

        let bottom_start = solid.vertex(solid.place(Point3::new(r, 0.0, 0.0)), "bottom-start")?;
        let pointed = top <= 0.0;
        let top_start = if pointed {
            None
        } else {
            Some(solid.vertex(solid.place(Point3::new(top, 0.0, h)), "top-start")?)
        };
        let bottom_centre = solid.vertex(solid.place(at(0.0)), "bottom-centre")?;
        let top_centre = solid.vertex(solid.place(at(h)), "top-centre")?;

        let rim = |solid: &SolidBuilder<'_>, z, radius| circle(solid.place(at(z)), x, y, radius);
        let bottom_rim = rim(&solid, 0.0, r)?;
        let bottom_rim = solid.edge(bottom_rim, bottom_start, bottom_start, "bottom-rim")?;
        let line = |solid: &SolidBuilder<'_>, a: Point3, b: Point3| {
            Curve3::line(solid.place(a), solid.place(b))
        };
        let seam = line(&solid, Point3::new(r, 0.0, 0.0), Point3::new(top, 0.0, h))?;
        let seam = solid.edge(seam, bottom_start, top_start.unwrap_or(top_centre), "seam")?;
        let bottom_seam = line(&solid, at(0.0), Point3::new(r, 0.0, 0.0))?;
        let bottom_seam = solid.edge(bottom_seam, bottom_centre, bottom_start, "bottom-seam")?;

        use Orientation::{Forward, Reversed};
        let uv = |u: f64, v: f64| Point2::new(u, v);
        // Each loop runs counter-clockwise around the (u, v) square.
        let side = ring_surface(&solid, [(r, 0.0), (top, h)])?;
        // (A cone comes to a point at its top: no rim there, and no
        // face.)
        let top_rim = match top_start {
            Some(start) => {
                let curve = rim(&solid, h, top)?;
                Some(solid.edge(curve, start, start, "top-rim")?)
            }
            None => None,
        };
        let side_loop = vec![
            solid.coedge(bottom_rim, Forward, uv(0.0, 0.0), uv(1.0, 0.0))?,
            solid.coedge(seam, Forward, uv(1.0, 0.0), uv(1.0, 1.0))?,
            match top_rim {
                Some(rim) => solid.coedge(rim, Reversed, uv(0.0, 1.0), uv(1.0, 1.0))?,
                None => solid.pole(top_centre, uv(1.0, 1.0), uv(0.0, 1.0))?,
            },
            solid.coedge(seam, Reversed, uv(0.0, 0.0), uv(0.0, 1.0))?,
        ];
        // The bottom runs from its centre (v = 0) out to the rim, so its
        // normal points down; the top from the rim in to its centre.
        let bottom = ring_surface(&solid, [(0.0, 0.0), (r, 0.0)])?;
        let bottom_loop = vec![
            solid.pole(bottom_centre, uv(0.0, 0.0), uv(1.0, 0.0))?,
            solid.coedge(bottom_seam, Forward, uv(1.0, 0.0), uv(1.0, 1.0))?,
            solid.coedge(bottom_rim, Reversed, uv(0.0, 1.0), uv(1.0, 1.0))?,
            solid.coedge(bottom_seam, Reversed, uv(0.0, 0.0), uv(0.0, 1.0))?,
        ];
        let mut faces = vec![
            solid.face(side, side_loop, "side")?,
            solid.face(bottom, bottom_loop, "bottom")?,
        ];
        if let (Some(top_rim), Some(top_start)) = (top_rim, top_start) {
            let top_seam = line(&solid, Point3::new(top, 0.0, h), at(h))?;
            let top_seam = solid.edge(top_seam, top_start, top_centre, "top-seam")?;
            let surface = ring_surface(&solid, [(top, h), (0.0, h)])?;
            let top_loop = vec![
                solid.coedge(top_rim, Forward, uv(0.0, 0.0), uv(1.0, 0.0))?,
                solid.coedge(top_seam, Forward, uv(1.0, 0.0), uv(1.0, 1.0))?,
                solid.pole(top_centre, uv(1.0, 1.0), uv(0.0, 1.0))?,
                solid.coedge(top_seam, Reversed, uv(0.0, 0.0), uv(0.0, 1.0))?,
            ];
            faces.push(solid.face(surface, top_loop, "top")?);
        }
        solid.solid(faces)?;
        Ok(())
    };
    Ok(workpiece_core::Made::body(id, shape))
}

#[cfg(test)]
mod tests {
    use super::*;
    use workpiece_core::Standalone;
    use workpiece_core::world::ORIGIN;
    use workpiece_core::world::topo::FaceId;

    #[test]
    fn cylinders_are_valid_named_solids() -> anyhow::Result<()> {
        let mut b = Standalone::new();
        let op = AddCylinder {
            id: "pin".into(),
            args: AddCylinderArgs {
                frame: Some(Reference::datum(ORIGIN)),
                ..AddCylinderArgs::default()
            },
        };
        op.apply(&mut b)?;
        let topo = &b.world.topo;
        assert_eq!(topo.validate(), Ok(()));
        assert_eq!(topo.faces().len(), 3);
        assert_eq!(topo.edges().len(), 5);
        // The top is a plane at the height, facing up.
        let top = b
            .world
            .resolve(&Reference::<workpiece_core::world::DatumPlane>::topology(
                "pin/top",
            ))?;
        assert!(top.normal().abs_diff_eq(Point3::Z, 1e-12));
        assert!((top.origin().z - 1.0).abs() < 1e-12);
        let bottom = b
            .world
            .resolve(&Reference::<workpiece_core::world::DatumPlane>::topology(
                "pin/bottom",
            ))?;
        assert!(bottom.normal().abs_diff_eq(-Point3::Z, 1e-12));
        assert!(topo.resolve::<FaceId>("pin/side").is_some());
        Ok(())
    }
}
