//! [`AddFrame`]: a datum frame, constructed in one of the common ways and
//! moved by a pose from there.

use std::collections::BTreeMap;

use workpiece_core::handles::Gizmo;
use workpiece_core::widgets::{PoseInput, VBox};
use workpiece_core::world::{DatumAxis, DatumFrame, DatumPlane};
use workpiece_core::{Builder, EditVisualization, Operation, Pose, Position, Source, World};

use crate::{
    Construction, Mode, construction_fields, ensure_free, fits, in_mode, pick, taken,
    with_references,
};

/// Size of the edit gizmo, in pixels.
const GIZMO_SIZE: f64 = 90.0;

/// How the frame is constructed, before its pose moves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FrameMode {
    /// At the pose alone, from the part's origin.
    #[default]
    AtPose,
    /// A plane's own frame: its origin and axes, z its normal.
    OnPlane,
    /// On a plane, at a point projected onto it: z the plane's normal, x
    /// the plane's x.
    PlanePoint,
    /// At the first point, x towards the second, y towards the third.
    ThreePoints,
    /// On an axis, z along it, x towards a point off it.
    AxisPoint,
    /// At a point (a vertex, another frame's origin), its axes the
    /// part's own: a point to build on.
    AtPoint,
}

impl Mode for FrameMode {
    const ALL: &'static [Self] = &[
        FrameMode::AtPose,
        FrameMode::OnPlane,
        FrameMode::PlanePoint,
        FrameMode::ThreePoints,
        FrameMode::AxisPoint,
        FrameMode::AtPoint,
    ];

    fn label(self) -> &'static str {
        match self {
            FrameMode::AtPose => "At pose",
            FrameMode::OnPlane => "On plane",
            FrameMode::PlanePoint => "On plane at point",
            FrameMode::ThreePoints => "Through 3 points",
            FrameMode::AxisPoint => "On axis towards point",
            FrameMode::AtPoint => "At point",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            FrameMode::AtPose => "frame-pose",
            FrameMode::OnPlane => "frame-plane",
            FrameMode::PlanePoint => "frame-plane-point",
            FrameMode::ThreePoints => "frame-3points",
            FrameMode::AxisPoint => "frame-axis-point",
            FrameMode::AtPoint => "frame-point",
        }
    }
}

/// The construction, its references, and the pose moving the frame from
/// what they construct.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct AddFrameArgs {
    /// How the frame is constructed.
    #[arg(with = "mode_fields")]
    pub mode: FrameMode,
    /// The references, in the order picked; the mode takes what it
    /// needs from them.
    #[arg(hidden)]
    pub references: Vec<workpiece_core::Reference<workpiece_core::Anything>>,
    /// Where the frame goes, relative to the constructed one (for
    /// [`FrameMode::AtPose`], to the part's origin).
    #[arg(with = "pose_field")]
    pub pose: Pose,
}

impl Default for AddFrameArgs {
    fn default() -> Self {
        Self {
            mode: FrameMode::default(),
            references: Vec::new(),
            pose: Pose::IDENTITY,
        }
    }
}

impl Construction for AddFrameArgs {
    type Mode = FrameMode;

    fn mode(&self) -> FrameMode {
        self.mode
    }

    fn references(&self) -> Vec<Source> {
        workpiece_core::Reference::sources(&self.references)
    }

    fn with(&self, mode: FrameMode, references: Vec<Source>) -> Self {
        let references = workpiece_core::Reference::each(references);
        Self {
            mode,
            references,
            ..self.clone()
        }
    }

    fn check(&self, world: &World) -> anyhow::Result<()> {
        self.base(world).map(|_| ())
    }
}

/// Adds a datum frame, named after the operation's id.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AddFrame {
    /// The operation's id; also the new datum's name.
    pub id: String,
    /// The construction and the pose.
    pub args: AddFrameArgs,
}

impl AddFrame {
    /// A frame at `pose` from the part's origin.
    pub fn at(id: impl Into<String>, pose: Pose) -> Self {
        Self {
            id: id.into(),
            args: AddFrameArgs {
                pose,
                ..AddFrameArgs::default()
            },
        }
    }

    /// The frame built from `selection` (in `world`): the references of a
    /// kind it takes, in the first mode they fit (else the default mode).
    pub fn from_selection(id: String, world: &World, selection: &[Source]) -> Self {
        let args = with_references(&AddFrameArgs::default(), world, taken(world, selection));
        Self { id, args }
    }

    /// Whether it uses all of `selection` and constructs a frame in
    /// `world`.
    pub fn fits(&self, world: &World, selection: &[Source]) -> bool {
        fits(&self.args, world, selection)
    }
}

impl AddFrameArgs {
    /// The constructed frame, before the pose moves it.
    fn base(&self, world: &World) -> anyhow::Result<Pose> {
        let r = &self.references;
        let degenerate = || anyhow::anyhow!("the references do not fix a frame");
        let axes = |origin, x: workpiece_core::math::Point3, y: workpiece_core::math::Point3| {
            let x = x.try_normalize().ok_or_else(degenerate)?;
            let y = y
                .reject_from_normalized(x)
                .try_normalize()
                .ok_or_else(degenerate)?;
            Ok::<_, anyhow::Error>(Pose::from_axes(origin, x, y))
        };
        match self.mode {
            FrameMode::AtPose => pick(world, r).map(|()| Pose::IDENTITY),
            FrameMode::OnPlane => {
                let (plane,): (DatumPlane,) = pick(world, r)?;
                Ok(plane.pose())
            }
            FrameMode::PlanePoint => {
                let (plane, point): (DatumPlane, Position) = pick(world, r)?;
                let n = plane.normal();
                let origin = point.0 - n * n.dot(point.0 - plane.origin());
                axes(origin, plane.x_dir(), plane.y_dir())
            }
            FrameMode::ThreePoints => {
                let (a, b, c): (Position, Position, Position) = pick(world, r)?;
                let (a, b, c) = (a.0, b.0, c.0);
                axes(a, b - a, c - a)
            }
            FrameMode::AtPoint => {
                let (point,): (Position,) = pick(world, r)?;
                Ok(Pose::from_translation(point.0))
            }
            FrameMode::AxisPoint => {
                let (axis, point): (DatumAxis, Position) = pick(world, r)?;
                let z = axis.direction();
                let origin = axis.origin() + z * z.dot(point.0 - axis.origin());
                let x = (point.0 - origin).try_normalize().ok_or_else(degenerate)?;
                axes(origin, x, z.cross(x))
            }
        }
    }
}

/// The numbers a frame is placed by, said of the model: how far it is
/// moved from what it is constructed on along each of that one's axes
/// it is moved along, and the angle it is turned by.
fn said(id: &str, base: Pose, pose: Pose) -> Vec<(String, workpiece_core::world::Annotation)> {
    use workpiece_core::math::Point3;
    use workpiece_core::world::Measured;
    let mut all = Vec::new();
    let moved = pose.translation();
    let from = base.transform_point(Point3::ZERO);
    for (k, what) in ["x", "y", "z"].into_iter().enumerate() {
        let by = moved[k];
        if by.abs() > 1e-12 {
            let measured = Measured::Length {
                from,
                to: from + base.transform_vector(Point3::AXES[k]) * by,
                along: None,
            };
            all.push(crate::said(id, what, measured, by.abs()));
        }
    }
    let (axis, angle) = pose.rotation().to_axis_angle();
    if angle.abs() > 1e-9 {
        let axis = base.transform_vector(axis);
        let first = axis.any_orthonormal_vector();
        let turned = glam::DQuat::from_axis_angle(axis, angle) * first;
        let measured = Measured::Angle {
            at: base.transform_point(moved),
            first,
            second: turned,
        };
        all.push(crate::said(id, "angle", measured, angle.to_degrees().abs()));
    }
    all
}

impl Operation for AddFrame {
    type Args = AddFrameArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &AddFrameArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut AddFrameArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // What it reads of the world: what it refers to.
        let world = builder.tracked();
        ensure_free(world, &self.id)?;
        let base = in_mode(self.args.mode, self.args.base(world))?;
        let pose = base * self.args.pose;
        let mut made = workpiece_core::Made::datum(&self.id, DatumFrame::from_pose(pose)?);
        made.annotations = said(&self.id, base, self.args.pose);
        Ok(made)
    }

    fn visualize_edit(
        &self,
        context: &workpiece_core::EditContext<'_>,
        (): &(),
    ) -> EditVisualization<workpiece_core::Edit<AddFrameArgs, ()>> {
        let world = context.world;
        let args = self.args.clone();
        // The fields the arguments give; the gizmo is its own.
        let fields = workpiece_core::Arguments::fields(&args, context);
        let with_pose = {
            let args = args.clone();
            move |pose| AddFrameArgs {
                pose,
                ..args.clone()
            }
        };
        // The gizmo sits on the frame itself; dragging it moves the pose
        // relative to the constructed frame.
        let mut handles = BTreeMap::new();
        if let Ok(base) = self.args.base(world) {
            let gizmo = Gizmo::new(base * args.pose, GIZMO_SIZE, move |p| {
                with_pose(base.inverse() * p)
            });
            handles.insert("pose".to_owned(), gizmo.into());
        }
        EditVisualization::new(VBox::new(fields), handles).map(workpiece_core::Edit::args)
    }

    fn summary(&self) -> String {
        let op = self;
        let t = op.args.pose.translation();
        let at = format!("({:.3}, {:.3}, {:.3})", t.x, t.y, t.z);
        match op.args.mode {
            crate::FrameMode::AtPose => format!("at {at}"),
            mode => format!(
                "{}, moved {at}",
                crate::constructed(
                    mode.label(),
                    &workpiece_core::Reference::sources(&op.args.references)
                )
            ),
        }
    }
}

/// The fields of the construction: its mode, with those that do not
/// construct from what is picked greyed out, and what is picked.
fn mode_fields(
    args: &AddFrameArgs,
    context: &workpiece_core::EditContext<'_>,
) -> Vec<workpiece_core::Widget<AddFrameArgs>> {
    construction_fields(args, context.world)
}

/// The field of where the frame is: its pose, or its offset from what
/// it is constructed on.
fn pose_field(
    args: &AddFrameArgs,
    _: &workpiece_core::EditContext<'_>,
) -> Vec<workpiece_core::Widget<AddFrameArgs>> {
    let label = if args.mode == FrameMode::AtPose {
        "Pose"
    } else {
        "Offset"
    };
    let with = args.clone();
    let field = PoseInput::new(label, args.pose, move |pose| AddFrameArgs {
        pose,
        ..with.clone()
    });
    vec![field.into()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use workpiece_core::handles::Handle;
    use workpiece_core::math::{Axis, Point3};
    use workpiece_core::world::{Datum, ORIGIN};
    use workpiece_core::{Standalone, Widget};

    fn pose_field(
        op: &AddFrame,
        world: &World,
    ) -> anyhow::Result<workpiece_core::widgets::PoseInput<AddFrameArgs>> {
        let Widget::VBox(fields) = workpiece_core::operation::edit_arguments(op, world).dialog
        else {
            anyhow::bail!("expected fields");
        };
        match fields.children.into_iter().nth(2) {
            Some(Widget::PoseInput(p)) => Ok(p),
            _ => anyhow::bail!("expected the pose"),
        }
    }

    #[test]
    fn frames_say_how_far_they_are_moved_and_turned() -> anyhow::Result<()> {
        use workpiece_core::Operation;
        let mut b = Standalone::new();
        let pose = Pose::from_rotation_translation(
            glam::DQuat::from_rotation_z(90f64.to_radians()),
            Point3::new(3.0, 0.0, -2.0),
        );
        AddFrame::at("f", pose).apply(&mut b)?;
        let said: Vec<(&str, String)> = b
            .world
            .annotations
            .iter()
            .map(|(name, note)| (name.as_str(), note.text()))
            .collect();
        assert_eq!(
            said,
            [
                ("f/angle", "90°".to_owned()),
                ("f/x", "3".to_owned()),
                ("f/z", "2".to_owned())
            ]
        );
        Ok(())
    }

    #[test]
    fn frames_sit_at_picked_points() -> anyhow::Result<()> {
        use workpiece_core::math::Point3;
        let mut world = World::new();
        let there = Pose::from_translation(Point3::new(1.0, 2.0, 3.0));
        let frame = workpiece_core::world::DatumFrame::from_pose(there)?;
        world.datums.insert("there".into(), frame.into());
        let args = AddFrameArgs {
            mode: FrameMode::AtPoint,
            references: vec![Source::Datum("there".into()).into()],
            pose: Pose::IDENTITY,
        };
        let at = args.base(&world)?.transform_point(Point3::ZERO);
        assert!(at.distance(Point3::new(1.0, 2.0, 3.0)) < 1e-12);
        Ok(())
    }

    #[test]
    fn frames_are_added_at_their_pose() -> anyhow::Result<()> {
        let mut b = Standalone::new();
        let pose = Pose::from_translation(Point3::new(1.0, 2.0, 3.0));
        let op = AddFrame::at("f", pose);
        op.apply(&mut b)?;
        let Some(Datum::Frame(f)) = b.world().datums.get("f") else {
            anyhow::bail!("no frame");
        };
        assert_eq!(f.origin(), Point3::new(1.0, 2.0, 3.0));
        assert!(
            AddFrame::at(ORIGIN, pose).apply(&mut b).is_err(),
            "the name is taken"
        );

        let edit = workpiece_core::operation::edit_arguments(&op, b.world());
        let Some(Handle::Gizmo(g)) = edit.handles.get("pose") else {
            anyhow::bail!("expected a gizmo");
        };
        assert_eq!(g.pose, pose);
        let fields = pose_field(&op, b.world())?;
        let moved = fields
            .set_position(Axis::Z, 7.0)
            .ok_or_else(|| anyhow::anyhow!("no edit"))?;
        assert!(
            moved
                .pose
                .translation()
                .abs_diff_eq(Point3::new(1.0, 2.0, 7.0), 1e-12)
        );
        assert!(
            fields.set_position(Axis::Z, f64::NAN).is_none(),
            "NaN input is ignored"
        );
        Ok(())
    }

    #[test]
    fn non_finite_poses_fail_to_apply() {
        let op = AddFrame::at(
            "f",
            Pose::from_translation(Point3::new(f64::INFINITY, 0.0, 0.0)),
        );
        assert!(op.apply(&mut Standalone::new()).is_err());
    }

    #[test]
    fn frames_are_constructed_and_offset() -> anyhow::Result<()> {
        let world = World::new();
        let plane = |axis| Source::FrameAxis {
            frame: ORIGIN.into(),
            axis,
        };
        let made = |selection: &[Source], offset: Pose| -> anyhow::Result<Pose> {
            let mut op = AddFrame::from_selection("f".into(), &world, selection);
            op.args.pose = offset;
            Ok(op.args.base(&world)? * op.args.pose)
        };
        // On the yz plane (normal x), lifted one along its normal.
        let lifted = made(&[plane(Axis::X)], Pose::from_translation(Point3::Z))?;
        assert!(lifted.translation().abs_diff_eq(Point3::X, 1e-12));
        assert!(lifted.axis(Axis::Z).abs_diff_eq(Point3::X, 1e-12));
        // The gizmo moves the offset, relative to the constructed frame.
        let op = AddFrame::from_selection("f".into(), &world, &[plane(Axis::X)]);
        assert_eq!(op.args.mode, FrameMode::OnPlane);
        let edit = workpiece_core::operation::edit_arguments(&op, &world);
        let Some(Handle::Gizmo(g)) = edit.handles.get("pose") else {
            anyhow::bail!("expected a gizmo");
        };
        let dragged = (g.on_drag)(Pose::from_translation(Point3::new(3.0, 0.0, 0.0)));
        assert!(
            dragged
                .pose
                .translation()
                .abs_diff_eq(Point3::new(0.0, 0.0, 3.0), 1e-12),
            "along the plane's normal"
        );
        Ok(())
    }
}
