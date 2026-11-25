//! Revolve: areas of a sketch swept around an axis in its plane.

use std::collections::BTreeMap;

use anyhow::Context;
use shapecraft_boolean::read;
use shapecraft_core::handles::AngularHandle;
use shapecraft_core::widgets::{Choice, ReferenceInput, VBox};
use shapecraft_core::world::{AreaPick, DatumAxis};
use shapecraft_core::{
    Builder, Edit, EditContext, EditVisualization, Measure, Operation, Reference, Source, Widget,
};

use shapecraft_shape::bodies::{self, Combine};
use shapecraft_shape::sweep::{self, Frame, Make, Turn};
use shapecraft_shape::ui;

/// The axis of a revolution.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum RevolveAxis {
    /// The sketch plane's u axis.
    #[default]
    SketchU,
    /// The sketch plane's v axis.
    SketchV,
    /// The rotation axis drawn in the sketch.
    Drawn,
    /// A picked axis: a datum axis, a frame's axis, or a straight edge,
    /// in the sketch's plane.
    Picked(Option<Reference<DatumAxis>>),
}

/// What to revolve, around what, how far, and what it becomes.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct RevolveArgs {
    /// The sketch.
    #[arg(with = "sketch_fields")]
    pub sketch: Option<shapecraft_core::Reference<shapecraft_core::Sketched>>,
    /// The areas of it to revolve, each by a point inside it or by its
    /// boundary; none for the whole sketch.
    #[serde(default)]
    pub areas: Vec<AreaPick>,
    /// The axis.
    #[serde(default)]
    #[arg(with = "axis_fields")]
    pub axis: RevolveAxis,
    /// The angle, in degrees; 360 or more is all the way around.
    #[arg(label = "Angle °")]
    pub angle: Measure,
    /// Whether it turns the other way around the axis.
    #[serde(default)]
    #[arg(flip, row, when = "!self.symmetric")]
    pub flip: bool,
    /// Whether the angle is half to either side of the sketch.
    #[serde(default)]
    #[arg(label = "Extent", options = "One side|Symmetric")]
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
    pub target: Option<shapecraft_core::Reference<shapecraft_core::world::topo::BodyId>>,
    /// Whether it makes a surface (a sheet) instead of a solid.
    #[serde(default)]
    #[arg(hidden)]
    pub surface: bool,
    /// Whether a surface's ends are closed with flat faces.
    #[serde(default)]
    #[arg(label = "Ends", options = "Open|Closed", when = "self.surface")]
    pub caps: bool,
}

impl Default for RevolveArgs {
    fn default() -> Self {
        Self {
            sketch: None,
            areas: Vec::new(),
            axis: RevolveAxis::SketchU,
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

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = shapecraft_shape::bodies::Out::default();
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
        let (origin, axis) = match &args.axis {
            RevolveAxis::SketchU => (frame.origin, frame.x),
            RevolveAxis::SketchV => (frame.origin, frame.y),
            RevolveAxis::Drawn => drawn_axis(&profile)
                .context("the sketch has no rotation axis drawn: draw one, or pick an axis")?,
            RevolveAxis::Picked(axis) => {
                let axis = world.resolve(axis.as_ref().context("pick an axis")?)?;
                (axis.origin(), axis.direction())
            }
        };
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
            use shapecraft_core::world::{Annotation, Measured};
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
        let mut scratch = shapecraft_core::world::topo::Topo::new();
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

    fn references(&self) -> Vec<Source> {
        let args = &self.args;
        let axis = match &args.axis {
            RevolveAxis::Picked(Some(axis)) => Some(&axis.source),
            _ => None,
        };
        // (The axis is in what says how the sketch is turned.)
        let mut out = shapecraft_core::Arguments::references(args);
        out.extend(axis.cloned());
        out
    }

    fn references_mut(&mut self) -> Vec<&mut Source> {
        let args = &mut self.args;
        let axis = match &mut args.axis {
            RevolveAxis::Picked(Some(axis)) => Some(&mut axis.source),
            _ => None,
        };
        let mut out: Vec<&mut Source> = args.sketch.iter_mut().map(|r| &mut r.source).collect();
        out.extend(args.target.iter_mut().map(|r| &mut r.source));
        out.extend(axis);
        out
    }

    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        (): &(),
    ) -> EditVisualization<Edit<RevolveArgs, ()>> {
        let (world, args) = (context.world, &self.args);
        // The fields the arguments give; the handles are its own.
        let fields = shapecraft_core::Arguments::fields(args, context);
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
            let axis = match &args.axis {
                RevolveAxis::SketchU => Some((frame.origin, frame.x)),
                RevolveAxis::SketchV => Some((frame.origin, frame.y)),
                RevolveAxis::Drawn => drawn_axis(profile),
                RevolveAxis::Picked(axis) => axis
                    .as_ref()
                    .and_then(|axis| world.resolve(axis).ok())
                    .map(|axis| (axis.origin(), axis.direction())),
            };
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

/// The field that picks the sketch (what was picked of another sketch
/// goes with it), and what of it is taken.
/// A sketch with a rotation axis drawn is turned about that, unless
/// another axis is picked.
fn sketch_fields(args: &RevolveArgs, context: &EditContext<'_>) -> Vec<Widget<RevolveArgs>> {
    let world = context.world.clone();
    let set = ui::set(
        args,
        move |a, sketch: Option<shapecraft_core::Reference<shapecraft_core::Sketched>>| {
            let drawn = sketch
                .as_ref()
                .and_then(|s| world.profiles.get(s.name()))
                .is_some_and(|p| p.axis.is_some());
            if drawn {
                a.axis = RevolveAxis::Drawn;
            } else if a.axis == RevolveAxis::Drawn {
                a.axis = RevolveAxis::SketchU;
            }
            a.sketch = sketch;
            a.areas.clear();
        },
    );
    ui::sketch_fields(&args.sketch, args.areas.len(), set)
}

/// The rotation axis drawn in `profile`'s sketch, if one is: a point it
/// passes through and its direction, in space.
fn drawn_axis(
    profile: &shapecraft_core::world::Profile,
) -> Option<(shapecraft_core::math::Point3, shapecraft_core::math::Point3)> {
    let (through, direction) = profile.axis?;
    let frame = Frame::of(profile);
    Some((
        frame.place(through),
        frame.x * direction.x + frame.y * direction.y,
    ))
}

/// The fields that say what it is turned around: the rotation axis
/// drawn in the sketch, an axis of the sketch's plane, or one picked.
fn axis_fields(args: &RevolveArgs, context: &EditContext<'_>) -> Vec<Widget<RevolveArgs>> {
    let axes = [
        "Sketch's rotation axis",
        "Sketch u axis",
        "Sketch v axis",
        "Picked axis",
    ]
    .map(str::to_owned)
    .to_vec();
    let selected = match args.axis {
        RevolveAxis::Drawn => 0,
        RevolveAxis::SketchU => 1,
        RevolveAxis::SketchV => 2,
        RevolveAxis::Picked(_) => 3,
    };
    let drawn = ui::profile(context.world, &args.sketch).is_some_and(|p| p.axis.is_some());
    let reason = if drawn {
        String::new()
    } else {
        "The sketch has no rotation axis drawn".to_owned()
    };
    let mut fields: Vec<Widget<RevolveArgs>> = Vec::new();
    fields.push(
        Choice::new(
            "Axis",
            axes,
            selected,
            ui::set(args, |a, i: usize| {
                a.axis = match i {
                    0 => RevolveAxis::Drawn,
                    1 => RevolveAxis::SketchU,
                    2 => RevolveAxis::SketchV,
                    _ => RevolveAxis::Picked(None),
                };
            }),
        )
        .drop_down()
        .with_enabled(vec![drawn || selected == 0, true, true, true])
        .with_reasons(vec![reason, String::new(), String::new(), String::new()])
        .into(),
    );
    if let RevolveAxis::Picked(axis) = &args.axis {
        fields.push(
            ReferenceInput::new(
                "Around",
                axis.clone(),
                ui::set(args, |a, axis| a.axis = RevolveAxis::Picked(Some(axis))),
            )
            .into(),
        );
    }
    fields
}
