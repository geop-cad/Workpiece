//! [`AddBox`]: a box centred on a frame.

use anyhow::Context;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use shapecraft_core::handles::LinearHandle;
use shapecraft_core::math::{Axis, Point2, Point3};
use shapecraft_core::widgets::VBox;
use shapecraft_core::world::DatumFrame;
use shapecraft_core::world::topo::{Curve3, NurbsSurface, Orientation};
use shapecraft_core::{Builder, EditVisualization, Measure, Operation, Reference, World};

use crate::brep::SolidBuilder;
use crate::frame_handles;

/// Where the box goes and how big it is.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct AddBoxArgs {
    /// The frame the box is centred on and aligned with.
    pub frame: Option<Reference<DatumFrame>>,
    /// The edge lengths along the frame's x, y and z axes.
    #[arg(field, label = "Width|Depth|Height")]
    pub size: [Measure; 3],
}

impl Default for AddBoxArgs {
    fn default() -> Self {
        Self {
            frame: None,
            size: [1.0; 3].map(Measure::new),
        }
    }
}

/// Adds a solid box, named after the operation's id: faces `{id}/top`
/// (+z), `bottom`, `front` (+x, towards the viewer of the front view),
/// `back`, `right` (+y) and `left`; edges `{id}/edge0`… and vertices
/// `{id}/vertex0`….
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AddBox {
    /// The operation's id; also the prefix of the box's names.
    pub id: String,
    /// The frame and the size.
    pub args: AddBoxArgs,
}

/// The faces, by name, as corner indices counter-clockwise seen from
/// outside. Corner `i` is at `(±x, ±y, ±z)` with bit 0, 1, 2 of `i`
/// choosing +x, +y, +z.
const FACES: [(&str, [usize; 4]); 6] = [
    ("bottom", [0, 2, 3, 1]),
    ("top", [4, 5, 7, 6]),
    ("left", [0, 1, 5, 4]),
    ("right", [2, 6, 7, 3]),
    ("back", [0, 4, 6, 2]),
    ("front", [1, 3, 7, 5]),
];

/// The (u, v) of a face's corners, in loop order.
const CORNER_UV: [Point2; 4] = [
    Point2::new(0.0, 0.0),
    Point2::new(1.0, 0.0),
    Point2::new(1.0, 1.0),
    Point2::new(0.0, 1.0),
];

impl Operation for AddBox {
    type Args = AddBoxArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &AddBoxArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut AddBoxArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        let lengths = ["the width", "the depth", "the height"];
        let mut size = Point3::ZERO;
        for (i, name) in lengths.into_iter().enumerate() {
            size[i] = (*builder).measure(&self.args.size[i], name)?;
        }
        anyhow::ensure!(
            size.is_finite() && size.min_element() > 0.0,
            "box size {size} is not positive"
        );
        let frame = self.args.frame.as_ref().context("pick a frame")?;
        // (It reads its frame, and no more; what it makes of that is
        // written by the builder.)
        let pose = builder.resolve(frame)?.pose();
        let shape = move |topo: &mut shapecraft_core::world::topo::Topo, name: &str| {
            let mut solid = SolidBuilder::new(topo, name, pose)?;

            let half = size / 2.0;
            let corner = |i: usize| {
                let sign = |bit: usize| if i & bit == 0 { -1.0 } else { 1.0 };
                Point3::new(sign(1) * half.x, sign(2) * half.y, sign(4) * half.z)
            };
            let points: Vec<Point3> = (0..8).map(|i| solid.place(corner(i))).collect();
            let mut vertices = Vec::with_capacity(8);
            for (i, &p) in points.iter().enumerate() {
                vertices.push(solid.vertex(p, &format!("vertex{i}"))?);
            }

            // Each edge is made once, from its lower to its higher corner, by
            // the first face that walks it.
            let mut edges = BTreeMap::new();
            let mut faces = Vec::with_capacity(6);
            for (name, corners) in FACES {
                let mut coedges = Vec::with_capacity(4);
                for k in 0..4 {
                    let (a, b) = (corners[k], corners[(k + 1) % 4]);
                    let (uv_a, uv_b) = (CORNER_UV[k], CORNER_UV[(k + 1) % 4]);
                    let (lo, hi) = (a.min(b), a.max(b));
                    let next = edges.len();
                    let edge = match edges.entry((lo, hi)) {
                        Entry::Occupied(e) => *e.get(),
                        Entry::Vacant(e) => {
                            let curve = Curve3::line(points[lo], points[hi])?;
                            let name = format!("edge{next}");
                            let id = solid.edge(curve, vertices[lo], vertices[hi], &name)?;
                            *e.insert(id)
                        }
                    };
                    // The pcurve runs from the edge's start (the lower corner).
                    let (orientation, from, to) = if a == lo {
                        (Orientation::Forward, uv_a, uv_b)
                    } else {
                        (Orientation::Reversed, uv_b, uv_a)
                    };
                    coedges.push(solid.coedge(edge, orientation, from, to)?);
                }
                let [p00, p10, p11, p01] = corners.map(|c| points[c]);
                let surface = NurbsSurface::bilinear(p00, p10, p01, p11)?;
                faces.push(solid.face(surface, coedges, name)?);
            }
            solid.solid(faces)?;
            Ok(())
        };
        Ok(shapecraft_core::Made::body(&self.id, shape))
    }

    fn visualize_edit(
        &self,
        context: &shapecraft_core::EditContext<'_>,
        (): &(),
    ) -> EditVisualization<shapecraft_core::Edit<AddBoxArgs, ()>> {
        let world: &World = context.world;
        let AddBoxArgs { frame, size } = self.args.clone();
        let with_size = {
            let (frame, size) = (frame.clone(), size.clone());
            move |axis: Axis, length: Measure| {
                let mut size = size.clone();
                size[axis.index()] = length;
                AddBoxArgs {
                    frame: frame.clone(),
                    size,
                }
            }
        };
        // The fields the arguments give; the handles are its own.
        let dialog = VBox::new(shapecraft_core::Arguments::fields(&self.args, context));
        // An arrow to the centre of each face the frame's axes point at;
        // none for a length a formula gives.
        let handles = frame_handles(world, &frame, &Axis::ALL, |axis, origin, direction| {
            let with_size = with_size.clone();
            let length = &size[axis.index()];
            let drag = move |half: f64| with_size(axis, Measure::new(half * 2.0));
            LinearHandle::new(origin, direction, length.value / 2.0, drag)
                .filter(|_| length.is_plain())
        });
        EditVisualization::new(dialog, handles).map(shapecraft_core::Edit::args)
    }

    fn summary(&self) -> String {
        let op = self;
        let s = &op.args.size;
        format!(
            "{} × {} × {} on {}",
            s[0],
            s[1],
            s[2],
            shapecraft_core::reference::described(&op.args.frame)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shapecraft_core::Standalone;
    use shapecraft_core::world::ORIGIN;
    use shapecraft_core::world::topo::{EdgeId, FaceId, VertexId};

    #[test]
    fn boxes_are_valid_named_solids() -> anyhow::Result<()> {
        let mut b = Standalone::new();
        let op = AddBox {
            id: "box1".into(),
            args: AddBoxArgs {
                size: [2.0, 4.0, 6.0].map(shapecraft_core::Measure::new),
                frame: Some(Reference::datum(ORIGIN)),
            },
        };
        op.apply(&mut b)?;
        let topo = &b.world.topo;
        assert_eq!(topo.validate(), Ok(()));
        assert_eq!(topo.faces().len(), 6);
        assert_eq!(topo.edges().len(), 12);
        assert_eq!(topo.vertices().len(), 8);
        assert!(topo.resolve::<FaceId>("box1/top").is_some());
        assert!(topo.resolve::<VertexId>("box1/vertex7").is_some());
        assert_eq!(
            topo.names()
                .iter()
                .filter(|(_, id)| EdgeId::try_from(*id).is_ok())
                .count(),
            12
        );
        // The top face is planar, facing +z, at half the height.
        let top = b
            .world
            .resolve(&Reference::<shapecraft_core::world::DatumPlane>::topology(
                "box1/top",
            ))?;
        assert!(top.normal().abs_diff_eq(Point3::Z, 1e-12));
        assert!((top.origin().z - 3.0).abs() < 1e-12);
        let face = |name: &str| {
            b.world
                .resolve(&Reference::<shapecraft_core::world::DatumPlane>::topology(
                    name,
                ))
        };
        assert!(face("box1/front")?.normal().abs_diff_eq(Point3::X, 1e-12));
        assert!(face("box1/right")?.normal().abs_diff_eq(Point3::Y, 1e-12));

        // The names are taken now, and sizes must be positive.
        assert!(op.apply(&mut b).is_err());
        let flat = AddBox {
            id: "flat".into(),
            args: AddBoxArgs {
                size: [1.0, 0.0, 1.0].map(shapecraft_core::Measure::new),
                frame: Some(Reference::datum(ORIGIN)),
            },
        };
        assert!(flat.apply(&mut b).is_err());
        Ok(())
    }

    #[test]
    fn handles_drag_the_size() -> anyhow::Result<()> {
        let op = AddBox {
            id: "box1".into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(ORIGIN)),
                ..AddBoxArgs::default()
            },
        };
        let edit = shapecraft_core::operation::edit_arguments(&op, &World::new());
        let Some(shapecraft_core::handles::Handle::Linear(z)) = edit.handles.get("z") else {
            anyhow::bail!("expected the z arrow");
        };
        assert!(z.tip().abs_diff_eq(Point3::new(0.0, 0.0, 0.5), 1e-12));
        let edited = (z.on_drag)(1.5);
        assert_eq!(edited.size, [1.0, 1.0, 3.0].map(Measure::new));
        Ok(())
    }
}
