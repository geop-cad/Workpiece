//! Revolve: areas of a sketch swept around an axis in its plane.

use std::collections::BTreeMap;

use anyhow::Context;
use workpiece_boolean::read;
use workpiece_core::handles::AngularHandle;
use workpiece_core::widgets::VBox;
use workpiece_core::world::{AreaPick, DatumAxis};
use workpiece_core::{
    Builder, Edit, EditContext, EditVisualization, Measure, Operation, Reference, Source, Widget,
};

use workpiece_shape::bodies::{self, Combine};
use workpiece_shape::sweep::{self, Frame, Make, Turn};
use workpiece_shape::ui;

/// What to revolve, around what, how far, and what it becomes.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct RevolveArgs {
    /// The sketch.
    #[arg(with = "sketch_fields")]
    pub sketch: Option<workpiece_core::Reference<workpiece_core::Sketched>>,
    /// The areas of it to revolve, each by a point inside it or by its
    /// boundary; none for the whole sketch.
    #[serde(default)]
    pub areas: Vec<AreaPick>,
    /// The axis it turns about: a rotation axis drawn in a sketch, a
    /// datum axis, a frame's axis, a straight edge, a round face's axis.
    #[serde(default)]
    #[arg(label = "Rotation axis")]
    pub axis: Option<Reference<DatumAxis>>,
    /// The angle, in degrees; 360 or more is all the way around.
    #[arg(label = "Angle °")]
    pub angle: Measure,
    /// Whether it turns the other way around the axis.
    #[serde(default)]
    #[arg(flip, row, when = "!self.symmetric")]
    pub flip: bool,
    /// Whether the angle is half to either side of the sketch.
    #[serde(default)]
    #[arg(
        label = "Extent",
        options = "One side|Symmetric",
        icons = "extent-distance|extent-symmetric"
    )]
    pub symmetric: bool,
    /// What the solid does to the bodies there are.
    #[serde(default)]
    #[arg(choice, label = "Result", when = "!self.surface")]
    pub combine: Combine,
    /// The body it is combined with; `None` for every solid body.
    #[serde(default)]
    #[arg(
        label = "With",
        empty = "All bodies",
        when = "!self.surface && self.combine != Combine::NewBody"
    )]
    pub target: Option<workpiece_core::Reference<workpiece_core::world::topo::BodyId>>,
    /// Whether it makes a surface (a sheet) instead of a solid.
    #[serde(default)]
    #[arg(hidden)]
    pub surface: bool,
    /// Whether a surface's ends are closed with flat faces.
    #[serde(default)]
    #[arg(
        label = "Ends",
        options = "Open|Closed",
        icons = "ends-open|ends-closed",
        when = "self.surface"
    )]
    pub caps: bool,
}

impl Default for RevolveArgs {
    fn default() -> Self {
        Self {
            sketch: None,
            areas: Vec::new(),
            axis: None,
            angle: Measure::new(360.0),
            symmetric: false,
            flip: false,
            combine: Combine::Join,
            target: None,
            surface: false,
            caps: false,
        }
    }
}

/// Sweeps areas of a sketch around an axis into a body named after the
/// operation's id: `{id}/side/{edge}` is the face each edge of the sketch
/// sweeps, `{id}/start` and `{id}/end` the flat ends of a part turn.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Revolve {
    /// The operation's id; also the name of the body it makes.
    pub id: String,
    /// What to revolve and how.
    pub args: RevolveArgs,
}

impl Revolve {
    /// A revolution of `sketch` (a sketch's id), as a solid or a surface.
    pub fn of(id: impl Into<String>, sketch: Option<String>, surface: bool) -> Self {
        Self {
            id: id.into(),
            args: RevolveArgs {
                sketch: sketch.map(|s| Source::Profile(s).into()),
                surface,
                ..RevolveArgs::default()
            },
        }
    }
}

impl Operation for Revolve {
    type Args = RevolveArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &RevolveArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut RevolveArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = workpiece_shape::bodies::Out::default();
        let args = &self.args;
        let degrees = (*builder).measure(&args.angle, "the angle")?;
        anyhow::ensure!(
            degrees.is_finite() && degrees > 0.0,
            "angle {degrees} is not positive"
        );
        let world = builder.tracked();
        let sketch = args.sketch.as_ref().context("pick a sketch")?.name();
        let profile = world
            .profiles
            .get(sketch)
            .with_context(|| format!("there is no sketch {sketch:?}"))?
            .clone();
        let frame = Frame::of(&profile);
        let axis = args.axis.as_ref().context(
            "pick the rotation axis: one drawn in the sketch, an axis or a straight edge",
        )?;
        let axis = world.resolve(axis)?;
        let (origin, axis) = (axis.origin(), axis.direction());
        let turn = Turn {
            origin,
            axis: axis.normalize() * if args.flip { -1.0 } else { 1.0 },
        };
        let regions = sweep::regions(&profile, &args.areas)?;
        anyhow::ensure!(
            args.surface || !regions.is_empty(),
            "the sketch has no closed area: only a surface can be made of it{}",
            profile.open_note()
        );
        let angle = degrees.to_radians().min(std::f64::consts::TAU);
        let start = if args.symmetric { -angle / 2.0 } else { 0.0 };
        let make = if args.surface {
            Make::Sheet { caps: args.caps }
        } else {
            Make::Solid
        };
        let topo = &world.topo;
        let targets = if args.surface {
            Vec::new()
        } else {
            bodies::targets(topo, &self.id, &args.target)?
        };
        let range = (start, angle);
        // How far around it goes, where that is not all the way, is a
        // size of the model.
        let middle = frame.place(ui::middle(&profile, &args.areas));
        let foot = turn.origin + turn.axis * (middle - turn.origin).dot(turn.axis);
        if angle < std::f64::consts::TAU - 1e-9
            && let Some(out_of) = (middle - foot).try_normalize()
        {
            use workpiece_core::world::{Annotation, Measured};
            let at = |a: f64| out_of * a.cos() + turn.axis.cross(out_of) * a.sin();
            let measured = Measured::Angle {
                at: foot,
                first: at(start),
                second: at(start + angle),
            };
            let reach = middle.distance(foot) * 1.3;
            let held = Annotation::held(measured, degrees, &args.angle, reach);
            out.made
                .annotations
                .push((format!("{}/angle", self.id), held));
        }
        if args.surface {
            // Every curve of the sketch, closed or left open.
            let sheet = sweep::sheet_of(&profile, &args.areas, &|scratch, walks, closed| {
                sweep::revolve(
                    scratch, &self.id, &profile, walks, closed, turn, range, make,
                )
            })?;
            out.write_sheet(&sheet, &self.id);
            return Ok(out.made);
        }
        // (Swept in a place of its own, then put with the bodies there are.)
        let walks = sweep::outline(&profile, &regions)?;
        let mut scratch = workpiece_core::world::topo::Topo::new();
        let body = sweep::revolve(
            &mut scratch,
            &self.id,
            &profile,
            &walks,
            true,
            turn,
            range,
            make,
        )?;
        if args.combine == Combine::NewBody {
            out.take(&scratch, body)?;
            return Ok(out.made);
        }
        let tool = read(&scratch, body)?;
        bodies::settle_into(topo, &mut out, &self.id, tool, args.combine, &targets)?;
        Ok(out.made)
    }

    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        (): &(),
    ) -> EditVisualization<Edit<RevolveArgs, ()>> {
        let (world, args) = (context.world, &self.args);
        // The fields the arguments give; the handles are its own.
        let fields = workpiece_core::Arguments::fields(args, context);
        let mut handles = BTreeMap::new();
        if let Some(profile) = ui::profile(world, &args.sketch) {
            let picker = ui::area_picker(
                profile,
                &args.areas,
                ui::set(args, |a, areas| a.areas = areas),
            );
            handles.insert("areas".to_owned(), picker);
            // A knob on a ring about the axis, from the middle of what
            // is swept: turning it sets the angle, and back through the
            // sketch turns it around.
            let frame = Frame::of(profile);
            let axis = args
                .axis
                .as_ref()
                .and_then(|axis| world.resolve(axis).ok())
                .map(|axis| (axis.origin(), axis.direction()));
            let value = args
                .angle
                .resolve(context.parameters)
                .unwrap_or(args.angle.value)
                .min(360.0);
            let knob = axis.and_then(|(origin, axis)| {
                let sign = if args.flip { -1.0 } else { 1.0 };
                let turn = Turn {
                    origin,
                    axis: axis.normalize() * sign,
                };
                let from = frame.place(ui::middle(profile, &args.areas));
                let center = turn.foot(from);
                // On the axis itself, any direction in the plane does.
                let zero = (from - center)
                    .try_normalize()
                    .unwrap_or_else(|| frame.normal().cross(turn.axis));
                if args.symmetric {
                    let drag = ui::set(args, |a, x: f64| {
                        a.angle = Measure::new((2.0 * x.abs()).min(360.0));
                    });
                    AngularHandle::new(center, turn.axis, zero, value / 2.0, drag)
                } else {
                    let drag = ui::set(args, |a, x: f64| {
                        a.flip ^= x < 0.0;
                        a.angle = Measure::new(x.abs().min(360.0));
                    });
                    AngularHandle::new(center, turn.axis, zero, value, drag)
                }
            });
            if let Some(knob) = knob {
                handles.insert("angle".to_owned(), knob.into());
            }
        }
        EditVisualization::new(VBox::new(fields), handles).map(Edit::args)
    }

    fn summary(&self) -> String {
        let op = self;
        let a = &op.args;
        let of = a.sketch.as_ref().map_or("(no sketch)", |s| s.name());
        let what = if a.surface {
            "surface"
        } else {
            a.combine.label()
        };
        format!("{of}, {}°, {}", a.angle, what.to_lowercase())
    }
}

/// The rotation axes drawn in the sketch `sketch` (its name), as they
/// are in `world`: the datum axes a sketch makes of them, by name.
pub fn drawn_axes(world: &workpiece_core::World, sketch: &str) -> Vec<String> {
    let prefix = format!("{sketch}/");
    world
        .datums
        .iter()
        .filter(|(name, datum)| {
            name.starts_with(&prefix) && matches!(datum, workpiece_core::world::Datum::Axis(_))
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// The field that picks the sketch (what was picked of another sketch
/// goes with it), and what of it is taken. A sketch with one rotation
/// axis drawn is turned about that, unless another axis is picked; the
/// axis drawn in a sketch picked before goes with that sketch.
fn sketch_fields(args: &RevolveArgs, context: &EditContext<'_>) -> Vec<Widget<RevolveArgs>> {
    let world = context.world.clone();
    let set = ui::set(
        args,
        move |a, sketch: Option<workpiece_core::Reference<workpiece_core::Sketched>>| {
            let before = a.sketch.as_ref().map(|s| format!("{}/", s.name()));
            let of_before = a.axis.as_ref().is_some_and(|axis| {
                matches!((&axis.source, &before), (Source::Datum(name), Some(b)) if name.starts_with(b))
            });
            if of_before {
                a.axis = None;
            }
            if let Some(sketch) = &sketch
                && let [only] = &drawn_axes(&world, sketch.name())[..]
            {
                a.axis = Some(Source::Datum(only.clone()).into());
            }
            a.sketch = sketch;
            a.areas.clear();
        },
    );
    ui::sketch_fields(&args.sketch, args.areas.len(), set)
}
