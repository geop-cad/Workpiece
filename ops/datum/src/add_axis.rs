//! [`AddAxis`]: a datum axis, constructed in one of the common ways.

use shapecraft_core::world::{DatumAxis, DatumPlane};
use shapecraft_core::{Builder, Operation, Position, Source, World};

use crate::add_plane::intersection;
use crate::{
    Construction, Mode, construction_fields, ensure_free, fits, in_mode, pick, taken,
    with_references,
};

/// How the axis is constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum AxisMode {
    /// Through two points, from the first.
    #[default]
    TwoPoints,
    /// Along what has an axis already: a straight edge, the axis of a
    /// round face (a cylinder's or a cone's wall), another axis.
    Along,
    /// Where two planes meet.
    PlanePlane,
    /// Through a point, normal to a plane.
    NormalToPlane,
    /// Through a point, parallel to an axis.
    ParallelThroughPoint,
}

impl Mode for AxisMode {
    const ALL: &'static [Self] = &[
        AxisMode::TwoPoints,
        AxisMode::Along,
        AxisMode::PlanePlane,
        AxisMode::NormalToPlane,
        AxisMode::ParallelThroughPoint,
    ];

    fn label(self) -> &'static str {
        match self {
            AxisMode::TwoPoints => "Through 2 points",
            AxisMode::Along => "Along an edge or a round face",
            AxisMode::PlanePlane => "Intersection of planes",
            AxisMode::NormalToPlane => "Normal to plane",
            AxisMode::ParallelThroughPoint => "Parallel through point",
        }
    }
}

/// The construction and its references.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    shapecraft_core::Arguments,
)]
pub struct AddAxisArgs {
    /// How the axis is constructed.
    #[arg(with = "mode_fields")]
    pub mode: AxisMode,
    /// The references, in the order picked; the mode takes what it
    /// needs from them.
    #[arg(hidden)]
    pub references: Vec<shapecraft_core::Reference<shapecraft_core::Anything>>,
}

impl Construction for AddAxisArgs {
    type Mode = AxisMode;

    fn mode(&self) -> AxisMode {
        self.mode
    }

    fn references(&self) -> Vec<Source> {
        shapecraft_core::Reference::sources(&self.references)
    }

    fn with(&self, mode: AxisMode, references: Vec<Source>) -> Self {
        let references = shapecraft_core::Reference::each(references);
        Self { mode, references }
    }

    fn check(&self, world: &World) -> anyhow::Result<()> {
        self.axis(world).map(|_| ())
    }
}

/// Adds a datum axis, named after the operation's id.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AddAxis {
    /// The operation's id; also the new datum's name.
    pub id: String,
    /// The construction.
    pub args: AddAxisArgs,
}

impl AddAxis {
    /// The axis built from `selection` (in `world`): the references of a
    /// kind it takes, in the first mode they fit (else the default mode).
    pub fn from_selection(id: String, world: &World, selection: &[Source]) -> Self {
        let args = with_references(&AddAxisArgs::default(), world, taken(world, selection));
        Self { id, args }
    }

    /// Whether it uses all of `selection` and constructs an axis in
    /// `world`.
    pub fn fits(&self, world: &World, selection: &[Source]) -> bool {
        fits(&self.args, world, selection)
    }
}

impl AddAxisArgs {
    /// The axis the references construct in `world`.
    fn axis(&self, world: &World) -> anyhow::Result<DatumAxis> {
        let r = &self.references;
        let degenerate = || anyhow::anyhow!("the references do not fix an axis");
        let axis = match self.mode {
            AxisMode::TwoPoints => {
                let (a, b): (Position, Position) = pick(world, r)?;
                DatumAxis::new(a.0, b.0 - a.0)
            }
            AxisMode::Along => {
                let (axis,): (DatumAxis,) = pick(world, r)?;
                return Ok(axis);
            }
            AxisMode::PlanePlane => {
                let (p, q): (DatumPlane, DatumPlane) = pick(world, r)?;
                return intersection(&p, &q)
                    .ok_or_else(|| anyhow::anyhow!("the planes are parallel"));
            }
            AxisMode::NormalToPlane => {
                let (p, point): (DatumPlane, Position) = pick(world, r)?;
                DatumAxis::new(point.0, p.normal())
            }
            AxisMode::ParallelThroughPoint => {
                let (a, point): (DatumAxis, Position) = pick(world, r)?;
                DatumAxis::new(point.0, a.direction())
            }
        };
        axis.map_err(|_| degenerate())
    }
}

impl Operation for AddAxis {
    type Args = AddAxisArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &AddAxisArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut AddAxisArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // What it reads of the world: what it refers to.
        let world = builder.tracked();
        ensure_free(world, &self.id)?;
        // (As in its dialog: where the mode it has cannot construct from
        // what it refers to and another can, that one does — an axis
        // said to go through two points and given a cylinder's wall.)
        let args = with_references(&self.args, world, self.args.references());
        let axis = in_mode(args.mode, args.axis(world))?;
        Ok(shapecraft_core::Made::datum(&self.id, axis))
    }

    fn summary(&self) -> String {
        let op = self;
        crate::constructed(
            op.args.mode.label(),
            &shapecraft_core::Reference::sources(&op.args.references),
        )
    }
}

/// The fields of the construction: its mode, with those that do not
/// construct from what is picked greyed out, and what is picked.
fn mode_fields(
    args: &AddAxisArgs,
    context: &shapecraft_core::EditContext<'_>,
) -> Vec<shapecraft_core::Widget<AddAxisArgs>> {
    construction_fields(args, context.world)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shapecraft_core::math::{Axis, Point3};
    use shapecraft_core::world::ORIGIN;

    #[test]
    fn axes_are_constructed_in_each_mode() -> anyhow::Result<()> {
        let world = World::new();
        let plane = |axis| Source::FrameAxis {
            frame: ORIGIN.into(),
            axis,
        };
        let make = |mode, selection: &[Source]| {
            let mut op = AddAxis::from_selection("a".into(), &world, selection);
            op.args.mode = mode;
            op.args.axis(&world)
        };
        // xy and yz meet in the y axis.
        let y = make(AxisMode::PlanePlane, &[plane(Axis::Z), plane(Axis::X)])?;
        assert!(y.direction().y.abs() > 1.0 - 1e-12 && y.origin().length() < 1e-12);
        let z = make(
            AxisMode::NormalToPlane,
            &[plane(Axis::Z), Source::Datum(ORIGIN.into())],
        )?;
        assert!(z.direction().abs_diff_eq(Point3::Z, 1e-12));
        assert!(
            make(AxisMode::PlanePlane, &[plane(Axis::Z), plane(Axis::Z)]).is_err(),
            "parallel"
        );
        Ok(())
    }

    #[test]
    fn a_round_face_has_an_axis_whatever_mode_the_step_is_in() -> anyhow::Result<()> {
        use shapecraft_core::{Measure, Reference, Standalone};
        use shapecraft_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
        let mut b = Standalone::new();
        AddCylinder {
            id: "rod".into(),
            args: AddCylinderArgs {
                frame: Some(Reference::datum(ORIGIN)),
                radius: Measure::new(5.0),
                height: Measure::new(20.0),
            },
        }
        .apply(&mut b)?;
        let wall = Source::Topology("rod/side".into());
        // Picked, the wall makes an axis along it: the rod's.
        let op = AddAxis::from_selection("a".into(), &b.world, std::slice::from_ref(&wall));
        assert_eq!(op.args.mode, AxisMode::Along);
        // And a step still said to go through two points builds too.
        AddAxis {
            id: "a".into(),
            args: AddAxisArgs {
                mode: AxisMode::TwoPoints,
                references: Reference::each(vec![wall]),
            },
        }
        .apply(&mut b)?;
        let Some(shapecraft_core::world::Datum::Axis(axis)) = b.world.datums.get("a") else {
            anyhow::bail!("expected an axis");
        };
        assert!(axis.direction().z.abs() > 1.0 - 1e-9);
        assert!(axis.origin().truncate().length() < 1e-9);
        Ok(())
    }
}
