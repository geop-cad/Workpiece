//! [`AddPlane`]: a datum plane, constructed in one of the common ways.

use std::collections::BTreeMap;

use workpiece_core::handles::{AngularHandle, LinearHandle};
use workpiece_core::math::Point3;
use workpiece_core::widgets::VBox;
use workpiece_core::world::{DatumAxis, DatumPlane};
use workpiece_core::{Builder, EditVisualization, Operation, Position, Source, World};

use crate::{
    Construction, Mode, construction_fields, ensure_free, fits, in_mode, pick, taken,
    with_references,
};

/// How the plane is constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum PlaneMode {
    /// Parallel to a plane, a distance along its normal.
    #[default]
    Offset,
    /// Halfway between two planes: parallel to them if they are, else
    /// through their intersection, the same distance from both.
    Midplane,
    /// Through an axis, turned an angle from a plane about it.
    Angle,
    /// Through three points.
    ThreePoints,
    /// Through an axis and a point.
    AxisPoint,
    /// Through a point, normal to an axis.
    NormalToAxis,
    /// Through a point, parallel to a plane.
    ParallelThroughPoint,
    /// Touching a face where a point is nearest it: tangent to a
    /// cylinder along the line through the point, to a ball at it.
    Tangent,
}

impl Mode for PlaneMode {
    const ALL: &'static [Self] = &[
        PlaneMode::Offset,
        PlaneMode::Midplane,
        PlaneMode::Angle,
        PlaneMode::ThreePoints,
        // Before the axis modes: a frame axis is also a plane, and through
        // a point parallel to it is the likelier intent.
        PlaneMode::ParallelThroughPoint,
        PlaneMode::NormalToAxis,
        PlaneMode::AxisPoint,
        PlaneMode::Tangent,
    ];

    fn label(self) -> &'static str {
        match self {
            PlaneMode::Offset => "Offset",
            PlaneMode::Midplane => "Midplane",
            PlaneMode::Angle => "At angle",
            PlaneMode::ThreePoints => "Through 3 points",
            PlaneMode::AxisPoint => "Through axis and point",
            PlaneMode::NormalToAxis => "Normal to axis",
            PlaneMode::ParallelThroughPoint => "Parallel through point",
            PlaneMode::Tangent => "Tangent to face at point",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            PlaneMode::Offset => "plane-offset",
            PlaneMode::Midplane => "plane-midplane",
            PlaneMode::Angle => "plane-angle",
            PlaneMode::ThreePoints => "plane-3points",
            PlaneMode::AxisPoint => "plane-axis-point",
            PlaneMode::NormalToAxis => "plane-normal-axis",
            PlaneMode::ParallelThroughPoint => "plane-parallel-point",
            PlaneMode::Tangent => "plane-tangent",
        }
    }
}

/// The construction and its references.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct AddPlaneArgs {
    /// How the plane is constructed.
    #[arg(with = "mode_fields")]
    pub mode: PlaneMode,
    /// The references, in the order picked; the mode takes what it
    /// needs from them.
    #[arg(hidden)]
    pub references: Vec<workpiece_core::Reference<workpiece_core::Anything>>,
    /// The offset's distance.
    #[arg(when = "self.mode == PlaneMode::Offset")]
    pub distance: f64,
    /// The angle in degrees.
    #[arg(label = "Angle °", when = "self.mode == PlaneMode::Angle")]
    pub angle: f64,
    /// Of two planes that cross, which of the two planes halfway
    /// between them: the one the two normals (as picked) lean towards
    /// alike, or the other, square to it.
    #[serde(default)]
    #[arg(
        label = "Bisector",
        options = "Between the normals|Across them",
        when = "self.mode == PlaneMode::Midplane"
    )]
    pub other: bool,
}

impl Default for AddPlaneArgs {
    fn default() -> Self {
        Self {
            mode: PlaneMode::default(),
            references: Vec::new(),
            distance: 1.0,
            angle: 45.0,
            other: false,
        }
    }
}

impl Construction for AddPlaneArgs {
    type Mode = PlaneMode;

    fn mode(&self) -> PlaneMode {
        self.mode
    }

    fn references(&self) -> Vec<Source> {
        workpiece_core::Reference::sources(&self.references)
    }

    fn with(&self, mode: PlaneMode, references: Vec<Source>) -> Self {
        let references = workpiece_core::Reference::each(references);
        Self {
            mode,
            references,
            ..self.clone()
        }
    }

    fn check(&self, world: &World) -> anyhow::Result<()> {
        self.plane(world).map(|_| ())
    }
}

/// Adds a datum plane, named after the operation's id.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AddPlane {
    /// The operation's id; also the new datum's name.
    pub id: String,
    /// The construction.
    pub args: AddPlaneArgs,
}

impl AddPlane {
    /// The plane built from `selection` (in `world`): the references of a
    /// kind it takes, in the first mode they fit (else the default mode).
    pub fn from_selection(id: String, world: &World, selection: &[Source]) -> Self {
        let args = with_references(&AddPlaneArgs::default(), world, taken(world, selection));
        Self { id, args }
    }

    /// Whether it uses all of `selection` and constructs a plane in
    /// `world`.
    pub fn fits(&self, world: &World, selection: &[Source]) -> bool {
        fits(&self.args, world, selection)
    }
}

impl AddPlaneArgs {
    /// The plane the references construct in `world`.
    fn plane(&self, world: &World) -> anyhow::Result<DatumPlane> {
        let a = self;
        let r = &a.references;
        let degenerate = || anyhow::anyhow!("the references do not fix a plane");
        let plane = match a.mode {
            PlaneMode::Offset => {
                let (base,): (DatumPlane,) = pick(world, r)?;
                DatumPlane::new(
                    base.origin() + base.normal() * a.distance,
                    base.normal(),
                    base.x_dir(),
                )
            }
            PlaneMode::Midplane => {
                let (p, q): (DatumPlane, DatumPlane) = pick(world, r)?;
                let (n1, n2) = (p.normal(), q.normal());
                if n1.cross(n2).length() < 1e-9 {
                    // Parallel: halfway along the first plane's normal.
                    let gap = (q.origin() - p.origin()).dot(n1);
                    DatumPlane::new(p.origin() + n1 * gap / 2.0, n1, p.x_dir())
                } else {
                    // The points as far from one plane as from the other:
                    // of the two such planes, the one the normals lean to
                    // alike (each as far to its side), or the other.
                    let line = intersection(&p, &q).ok_or_else(degenerate)?;
                    let normal = if a.other { n1 + n2 } else { n1 - n2 };
                    DatumPlane::new(line.origin(), normal, line.direction())
                }
            }
            PlaneMode::Angle => {
                let (p, axis): (DatumPlane, DatumAxis) = pick(world, r)?;
                let d = axis.direction();
                let start = p
                    .normal()
                    .reject_from_normalized(d)
                    .try_normalize()
                    .ok_or_else(degenerate)?;
                let turn = glam::DQuat::from_axis_angle(d, a.angle.to_radians());
                DatumPlane::new(axis.origin(), turn * start, d)
            }
            PlaneMode::ThreePoints => {
                let (a, b, c): (Position, Position, Position) = pick(world, r)?;
                let (a, b, c) = (a.0, b.0, c.0);
                DatumPlane::new(a, (b - a).cross(c - a), b - a)
            }
            PlaneMode::AxisPoint => {
                let (axis, point): (DatumAxis, Position) = pick(world, r)?;
                let d = axis.direction();
                DatumPlane::new(axis.origin(), d.cross(point.0 - axis.origin()), d)
            }
            PlaneMode::NormalToAxis => {
                let (axis, point): (DatumAxis, Position) = pick(world, r)?;
                let d = axis.direction();
                DatumPlane::new(point.0, d, d.any_orthonormal_vector())
            }
            PlaneMode::ParallelThroughPoint => {
                let (p, point): (DatumPlane, Position) = pick(world, r)?;
                DatumPlane::new(point.0, p.normal(), p.x_dir())
            }
            PlaneMode::Tangent => {
                let (face, point): (workpiece_core::world::topo::FaceId, Position) =
                    pick(world, r)?;
                let topo = &world.topo;
                let surface = topo
                    .face(face)
                    .and_then(|f| topo.surface(f.surface))
                    .ok_or_else(degenerate)?;
                // Where on the face's surface the point is nearest: on a
                // grid of it, then closer and closer around the best.
                let ((u0, u1), (v0, v1)) = surface.domain();
                let (mut lo, mut hi) = ((u0, v0), (u1, v1));
                let mut best = (f64::INFINITY, u0, v0);
                for _ in 0..24 {
                    for i in 0..=16 {
                        for j in 0..=16 {
                            let u = lo.0 + (hi.0 - lo.0) * f64::from(i) / 16.0;
                            let v = lo.1 + (hi.1 - lo.1) * f64::from(j) / 16.0;
                            let far = surface.point_at(u, v).distance(point.0);
                            if far < best.0 {
                                best = (far, u, v);
                            }
                        }
                    }
                    let (du, dv) = ((hi.0 - lo.0) / 8.0, (hi.1 - lo.1) / 8.0);
                    lo = ((best.1 - du).max(u0), (best.2 - dv).max(v0));
                    hi = ((best.1 + du).min(u1), (best.2 + dv).min(v1));
                }
                let (at, across, along) = surface.derivatives_at(best.1, best.2);
                // Facing out of the body, its axes laid out as a face's
                // plane is.
                let outwards = if topo.faces_outwards(face) == Some(false) {
                    -1.0
                } else {
                    1.0
                };
                DatumPlane::laid_out(at, across.cross(along) * outwards)
            }
        };
        plane.map_err(|_| degenerate())
    }
}

impl AddPlaneArgs {
    /// The number the plane is constructed by, said of the model: an
    /// offset plane's distance from the plane it is offset from, a
    /// plane at an angle its angle from the plane it is turned from.
    /// Nothing of the modes no number constructs.
    fn said(&self, world: &World, id: &str) -> Vec<(String, workpiece_core::world::Annotation)> {
        use workpiece_core::world::Measured;
        let r = &self.references;
        match self.mode {
            PlaneMode::Offset if self.distance != 0.0 => pick::<(DatumPlane,)>(world, r)
                .ok()
                .map(|(base,)| {
                    let from = base.origin();
                    let to = from + base.normal() * self.distance;
                    let measured = Measured::Length {
                        from,
                        to,
                        along: None,
                    };
                    crate::said(id, "distance", measured, self.distance.abs())
                })
                .into_iter()
                .collect(),
            PlaneMode::Angle => pick::<(DatumPlane, DatumAxis)>(world, r)
                .ok()
                .and_then(|(p, axis)| {
                    // (Between the two planes, each seen along the axis:
                    // the way across it in each.)
                    let d = axis.direction();
                    let start = p.normal().reject_from_normalized(d).try_normalize()?;
                    let turn = glam::DQuat::from_axis_angle(d, self.angle.to_radians());
                    let measured = Measured::Angle {
                        at: axis.origin(),
                        first: d.cross(start),
                        second: d.cross(turn * start),
                    };
                    Some(crate::said(id, "angle", measured, self.angle.abs()))
                })
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// The line where two planes meet, along `p.normal × q.normal`; `None`
/// for parallel planes.
pub(crate) fn intersection(p: &DatumPlane, q: &DatumPlane) -> Option<DatumAxis> {
    let (n1, n2) = (p.normal(), q.normal());
    let d = n1.cross(n2);
    let (c1, c2, k) = (n1.dot(p.origin()), n2.dot(q.origin()), n1.dot(n2));
    let det = 1.0 - k * k;
    if d.length() < 1e-9 || det.abs() < 1e-12 {
        return None;
    }
    let origin: Point3 = (n1 * (c1 - c2 * k) + n2 * (c2 - c1 * k)) / det;
    DatumAxis::new(origin, d).ok()
}

impl Operation for AddPlane {
    type Args = AddPlaneArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &AddPlaneArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut AddPlaneArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // What it reads of the world: what it refers to.
        let world = builder.tracked();
        ensure_free(world, &self.id)?;
        let plane = in_mode(self.args.mode, self.args.plane(world))?;
        let mut made = workpiece_core::Made::datum(&self.id, plane);
        made.annotations = self.args.said(world, &self.id);
        Ok(made)
    }

    fn visualize_edit(
        &self,
        context: &workpiece_core::EditContext<'_>,
        (): &(),
    ) -> EditVisualization<workpiece_core::Edit<AddPlaneArgs, ()>> {
        let world = context.world;
        let args = self.args.clone();
        // The fields the arguments give; the handles are its own.
        let fields = workpiece_core::Arguments::fields(&args, context);
        let mut handles = BTreeMap::new();
        match args.mode {
            PlaneMode::Offset => {
                let with_distance = {
                    let args = args.clone();
                    move |distance| AddPlaneArgs {
                        distance,
                        ..args.clone()
                    }
                };
                // An arrow along the base plane's normal, while it resolves.
                let base = pick::<(DatumPlane,)>(world, &args.references).ok();
                let arrow = base.and_then(|(b,)| {
                    LinearHandle::new(b.origin(), b.normal(), args.distance, with_distance)
                });
                handles.extend(arrow.map(|h| ("distance".to_owned(), h.into())));
            }
            PlaneMode::Angle => {
                let with_angle = {
                    let args = args.clone();
                    move |angle| AddPlaneArgs {
                        angle,
                        ..args.clone()
                    }
                };
                // A knob turning about the axis, from the plane's normal.
                let base = pick::<(DatumPlane, DatumAxis)>(world, &args.references).ok();
                let knob = base.and_then(|(plane, axis)| {
                    AngularHandle::new(
                        axis.origin(),
                        axis.direction(),
                        plane.normal(),
                        args.angle,
                        with_angle,
                    )
                });
                handles.extend(knob.map(|h| ("angle".to_owned(), h.into())));
            }
            _ => {}
        }
        EditVisualization::new(VBox::new(fields), handles).map(workpiece_core::Edit::args)
    }

    fn summary(&self) -> String {
        let op = self;
        crate::constructed(
            op.args.mode.label(),
            &workpiece_core::Reference::sources(&op.args.references),
        )
    }
}

/// The fields of the construction: its mode, with those that do not
/// construct from what is picked greyed out, and what is picked.
fn mode_fields(
    args: &AddPlaneArgs,
    context: &workpiece_core::EditContext<'_>,
) -> Vec<workpiece_core::Widget<AddPlaneArgs>> {
    construction_fields(args, context.world)
}

#[cfg(test)]
mod tests {
    use super::*;
    use workpiece_core::math::Axis;
    use workpiece_core::world::{DatumFrame, ORIGIN};
    use workpiece_core::{Pose, Standalone, Widget};

    fn frame_plane(axis: Axis) -> Source {
        Source::FrameAxis {
            frame: ORIGIN.into(),
            axis,
        }
    }

    fn build(
        mode: PlaneMode,
        world: &World,
        selection: &[Source],
        tweak: impl FnOnce(&mut AddPlaneArgs),
    ) -> anyhow::Result<DatumPlane> {
        let mut op = AddPlane::from_selection("p".into(), world, selection);
        op.args.mode = mode;
        tweak(&mut op.args);
        op.args.plane(world)
    }

    #[test]
    fn planes_say_the_number_they_are_set_by() -> anyhow::Result<()> {
        use workpiece_core::Operation;
        use workpiece_core::world::{Measured, Note};
        // Offset 4 below the origin's xy plane: a length of 4, from the
        // plane to it.
        let mut b = Standalone::new();
        AddPlane {
            id: "down".into(),
            args: AddPlaneArgs {
                mode: PlaneMode::Offset,
                references: vec![frame_plane(Axis::Z).into()],
                distance: -4.0,
                ..Default::default()
            },
        }
        .apply(&mut b)?;
        let said = &b.world.annotations["down/distance"];
        assert_eq!(said.text(), "4");
        let Note::Dimension {
            measured: Measured::Length { from, to, .. },
            ..
        } = &said.note
        else {
            panic!("a length: {said:?}");
        };
        assert!(from.distance(Point3::ZERO) < 1e-12);
        assert!(to.distance(Point3::new(0.0, 0.0, -4.0)) < 1e-12);
        // Turned 30° about the z axis from the xz plane: an angle of 30.
        let mut b = Standalone::new();
        AddPlane {
            id: "turned".into(),
            args: AddPlaneArgs {
                mode: PlaneMode::Angle,
                references: vec![
                    frame_plane(Axis::Y).into(),
                    Source::FrameAxis {
                        frame: ORIGIN.into(),
                        axis: Axis::Z,
                    }
                    .into(),
                ],
                angle: 30.0,
                ..Default::default()
            },
        }
        .apply(&mut b)?;
        let said = &b.world.annotations["turned/angle"];
        assert_eq!(said.text(), "30°");
        let Note::Dimension {
            measured: Measured::Angle { first, second, .. },
            ..
        } = &said.note
        else {
            panic!("an angle: {said:?}");
        };
        assert!((first.angle_between(*second).to_degrees() - 30.0).abs() < 1e-9);
        // (Nothing else is said of it.)
        assert!(
            b.world
                .annotations
                .keys()
                .all(|name| name.starts_with("turned/"))
        );
        Ok(())
    }

    #[test]
    fn planes_made_from_a_face_are_laid_out_as_the_face_is() -> anyhow::Result<()> {
        use workpiece_core::{Measure, Operation, Reference, Standalone};
        use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs};
        let mut b = Standalone::new();
        AddBox {
            id: "box".into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(workpiece_core::world::ORIGIN)),
                size: [2.0, 3.0, 4.0].map(Measure::new),
            },
        }
        .apply(&mut b)?;
        let (mut sides, mut tops) = (0, 0);
        for side in ["top", "bottom", "left", "right", "front", "back"] {
            let face = Source::Topology(format!("box/{side}"));
            let on: DatumPlane = workpiece_core::Resolve::resolve(&b.world, &face)?;
            // Offset: the face's normal, u and v, moved along the normal.
            let offset = build(
                PlaneMode::Offset,
                &b.world,
                std::slice::from_ref(&face),
                |a| {
                    a.distance = -1.5;
                },
            )?;
            assert!(offset.normal().abs_diff_eq(on.normal(), 1e-12), "{side}");
            assert!(offset.x_dir().abs_diff_eq(on.x_dir(), 1e-12), "{side}");
            assert!(offset.y_dir().abs_diff_eq(on.y_dir(), 1e-12), "{side}");
            // The rule: v is world +z in the plane, so up is up on a
            // side face; a top face has u = x, v = y, a bottom face
            // u = x, v = −y.
            let n = on.normal();
            if n.z.abs() < 0.5 {
                assert!(on.y_dir().dot(Point3::Z) > 0.0, "{side}");
                assert!(on.x_dir().abs_diff_eq(on.y_dir().cross(n), 1e-12), "{side}");
                assert!(offset.y_dir().dot(Point3::Z) > 0.0, "{side}");
                sides += 1;
            } else {
                assert!(on.x_dir().abs_diff_eq(Point3::X, 1e-12), "{side}");
                assert!(
                    on.y_dir().abs_diff_eq(Point3::Y * n.z.signum(), 1e-12),
                    "{side}"
                );
                tops += 1;
            }
            if n.abs_diff_eq(-Point3::X, 1e-12) {
                assert!(offset.x_dir().abs_diff_eq(-Point3::Y, 1e-12));
                assert!(offset.y_dir().abs_diff_eq(Point3::Z, 1e-12));
            }
            let moved = on.origin() + on.normal() * -1.5;
            assert!(offset.origin().abs_diff_eq(moved, 1e-12), "{side}");
            // Through a point, and half way to another face: as the face.
            let through = Source::Datum("out".into());
            let pose = Pose::from_translation(Point3::new(0.5, 0.25, 7.0));
            b.world
                .datums
                .insert("out".into(), DatumFrame::from_pose(pose)?.into());
            let parallel = build(
                PlaneMode::ParallelThroughPoint,
                &b.world,
                &[face.clone(), through],
                |_| (),
            )?;
            assert!(parallel.normal().abs_diff_eq(on.normal(), 1e-12), "{side}");
            assert!(parallel.x_dir().abs_diff_eq(on.x_dir(), 1e-12), "{side}");
        }
        assert_eq!((sides, tops), (4, 2));
        Ok(())
    }

    #[test]
    fn planes_touch_faces_where_a_point_is_nearest() -> anyhow::Result<()> {
        use workpiece_core::{Measure, Operation, Reference, Standalone};
        use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
        // A rod of radius 0.5 on the origin, and a frame out along x,
        // half way up: the plane touches the rod there, facing x.
        let mut b = Standalone::new();
        AddCylinder {
            id: "rod".into(),
            args: AddCylinderArgs {
                frame: Some(Reference::datum(workpiece_core::world::ORIGIN)),
                radius: Measure::new(0.5),
                height: Measure::new(2.0),
            },
        }
        .apply(&mut b)?;
        let pose = workpiece_core::Pose::from_translation(Point3::new(3.0, 0.0, 1.0));
        let frame = workpiece_core::world::DatumFrame::from_pose(pose)?;
        b.world.datums.insert("out".into(), frame.into());
        let picked = [
            Source::Topology("rod/side".into()),
            Source::Datum("out".into()),
        ];
        let plane = build(PlaneMode::Tangent, &b.world, &picked, |_| {})?;
        assert!(
            plane.origin().distance(Point3::new(0.5, 0.0, 1.0)) < 1e-6,
            "{:?}",
            plane.origin()
        );
        // Facing out of the rod, u laid out as a face's plane is.
        assert!(plane.normal().abs_diff_eq(Point3::X, 1e-6));
        assert!(plane.x_dir().abs_diff_eq(Point3::Y, 1e-6));
        Ok(())
    }

    #[test]
    fn planes_are_constructed_in_each_mode() -> anyhow::Result<()> {
        let mut world = World::new();
        let up = DatumFrame::from_pose(Pose::from_translation(Point3::new(0.0, 0.0, 4.0)))?;
        world.datums.insert("up".into(), up.into());
        let xy = frame_plane(Axis::Z);
        let xy_up = Source::FrameAxis {
            frame: "up".into(),
            axis: Axis::Z,
        };

        let offset = build(PlaneMode::Offset, &world, std::slice::from_ref(&xy), |a| {
            a.distance = 2.0
        })?;
        assert!((offset.origin().z - 2.0).abs() < 1e-12);
        // Parallel planes: halfway.
        let mid = build(
            PlaneMode::Midplane,
            &world,
            &[xy.clone(), xy_up.clone()],
            |_| (),
        )?;
        assert!((mid.origin().z - 2.0).abs() < 1e-12 && mid.normal().abs_diff_eq(Point3::Z, 1e-12));
        // Crossing planes (xy and yz): through the y axis, at 45°.
        let mid = build(
            PlaneMode::Midplane,
            &world,
            &[xy.clone(), frame_plane(Axis::X)],
            |_| (),
        )?;
        assert!(
            mid.normal().x.abs() > 0.7
                && mid.normal().z.abs() > 0.7
                && mid.origin().length() < 1e-12
        );
        // The one the normals (z, x) lean towards alike: x = z; or the
        // other, x = -z.
        let n = mid.normal();
        assert!((n.x + n.z).abs() < 1e-12, "{n}");
        let other = build(
            PlaneMode::Midplane,
            &world,
            &[xy.clone(), frame_plane(Axis::X)],
            |a| a.other = true,
        )?;
        let n = other.normal();
        assert!((n.x - n.z).abs() < 1e-12 && n.x.abs() > 0.7, "{n}");
        // About the x axis, 90° from xy: the xz plane.
        let turned = build(
            PlaneMode::Angle,
            &world,
            &[xy.clone(), frame_plane(Axis::X)],
            |a| a.angle = 90.0,
        )?;
        assert!(turned.normal().y.abs() > 1.0 - 1e-12);
        let three = build(
            PlaneMode::ThreePoints,
            &world,
            &[Source::Datum(ORIGIN.into()), Source::Datum("up".into())],
            |_| (),
        );
        let error = three.expect_err("two points are too few");
        assert!(
            format!("{error:#}").contains("needs 3 points. Picked: 2 points."),
            "{error:#}"
        );
        let normal = build(
            PlaneMode::NormalToAxis,
            &world,
            &[frame_plane(Axis::Z), Source::Datum("up".into())],
            |_| (),
        )?;
        assert!(
            (normal.origin().z - 4.0).abs() < 1e-12
                && normal.normal().abs_diff_eq(Point3::Z, 1e-12)
        );
        Ok(())
    }

    #[test]
    fn selections_choose_the_first_mode_they_fit() {
        let world = World::new();
        let xy = frame_plane(Axis::Z);
        let origin = Source::Datum(ORIGIN.into());
        let mode = |selection: &[Source]| {
            AddPlane::from_selection("p".into(), &world, selection)
                .args
                .mode
        };
        assert_eq!(mode(&[xy.clone(), xy.clone()]), PlaneMode::Midplane);
        assert_eq!(
            mode(&[origin.clone(), xy.clone()]),
            PlaneMode::ParallelThroughPoint
        );
        assert_eq!(mode(std::slice::from_ref(&xy)), PlaneMode::Offset);
        // Nothing fits one point: the default mode, waiting for more.
        assert_eq!(mode(&[origin]), PlaneMode::Offset);
    }

    #[test]
    fn the_mode_follows_the_references() {
        let world = World::new();
        let xy = frame_plane(Axis::Z);
        let args = AddPlaneArgs {
            mode: PlaneMode::Offset,
            references: vec![xy.clone().into()],
            ..AddPlaneArgs::default()
        };
        let op = AddPlane {
            id: "p".into(),
            args,
        };
        let edit = workpiece_core::operation::edit_arguments(&op, &world);
        let Widget::VBox(fields) = edit.dialog else {
            panic!("expected fields");
        };
        let [Widget::ReferenceList(list), Widget::Choice(modes), ..] = &fields.children[..] else {
            panic!("expected the references and the modes");
        };
        // One plane: offset fits, midplane does not.
        let enabled =
            |m: PlaneMode| modes.enabled[PlaneMode::ALL.iter().position(|&x| x == m).unwrap_or(0)];
        assert!(enabled(PlaneMode::Offset) && !enabled(PlaneMode::Midplane));
        // The modes that do not fit say why.
        let midplane = PlaneMode::ALL
            .iter()
            .position(|&m| m == PlaneMode::Midplane);
        assert_eq!(
            modes.reason(midplane.unwrap_or(0)),
            Some("Midplane: needs 2 planes. Picked: an axis or plane.")
        );
        assert_eq!(modes.reason(0), None);
        // Another plane: the offset no longer fits; the mode moves on.
        let two = list.toggle(frame_plane(Axis::X));
        assert_eq!(two.mode, PlaneMode::Midplane);
        assert_eq!(two.references.len(), 2);
        // Picking the first one again removes it.
        let one = list.toggle(xy);
        assert!(one.references.is_empty());
    }

    #[test]
    fn missing_references_say_what_to_pick() {
        let mut b = Standalone::new();
        let op = AddPlane {
            id: "p".into(),
            args: AddPlaneArgs::default(),
        };
        let error = op.apply(&mut b).expect_err("nothing picked");
        assert!(
            format!("{error:#}").contains("Offset: needs a plane. Picked: nothing."),
            "{error:#}"
        );
    }
}
