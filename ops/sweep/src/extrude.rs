//! Extrude: areas of a sketch swept along its plane's normal.

use std::collections::BTreeMap;

use anyhow::Context;
use shapecraft_boolean::read;
use shapecraft_core::handles::LinearHandle;
use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::widgets::{HBox, Label, Mood, NumericInput, ReferenceInput, VBox};
use shapecraft_core::world::AreaPick;
use shapecraft_core::world::topo::FaceId;
use shapecraft_core::{
    Builder, Edit, EditContext, EditVisualization, Measure, Operation, Reference, Source, Widget,
};

use shapecraft_shape::bodies::{self, Combine};
use shapecraft_shape::sweep::{self, Frame, Make};
use shapecraft_shape::ui;

/// Where an extrusion ends.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Extent {
    /// A distance from the sketch.
    #[default]
    Distance,
    /// A distance, half of it to either side of the sketch.
    Symmetric,
    /// Beyond every body there is.
    ThroughAll,
    /// Through everything, both ways from the sketch.
    ThroughBoth,
    /// Where it first runs into a body (or, cutting, first leaves one).
    ToNext,
    /// At a picked face.
    ToFace,
}

impl Extent {
    /// The extents of one that goes one way only, as a bore does.
    pub const ONE_WAY: [Extent; 4] = [
        Extent::Distance,
        Extent::ThroughAll,
        Extent::ToNext,
        Extent::ToFace,
    ];

    const ALL: [Extent; 6] = [
        Extent::Distance,
        Extent::Symmetric,
        Extent::ThroughAll,
        Extent::ThroughBoth,
        Extent::ToNext,
        Extent::ToFace,
    ];

    fn label(self) -> &'static str {
        match self {
            Extent::Distance => "Distance",
            Extent::Symmetric => "Symmetric",
            Extent::ThroughAll => "Through all",
            Extent::ThroughBoth => "Through all, both ways",
            Extent::ToNext => "To next",
            Extent::ToFace => "To face",
        }
    }
}

/// What to extrude, how far, and what it becomes.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct ExtrudeArgs {
    /// The sketch.
    #[arg(with = "sketch_fields")]
    pub sketch: Option<shapecraft_core::Reference<shapecraft_core::Sketched>>,
    /// The areas of it to extrude, each by a point inside it or by its
    /// boundary; none for the whole sketch (areas inside others are
    /// holes).
    #[serde(default)]
    pub areas: Vec<AreaPick>,
    /// Where it ends.
    #[serde(default)]
    #[arg(among = "Extent::ALL[..if self.surface { 2 } else { Extent::ALL.len() }]")]
    pub extent: Extent,
    /// The distance, for the extents that have one.
    #[arg(with = "how_far")]
    pub distance: Measure,
    /// Whether it goes against the sketch plane's normal.
    #[serde(default)]
    #[arg(hidden)]
    pub flip: bool,
    /// The face it ends at, for [`Extent::ToFace`].
    #[serde(default)]
    #[arg(hidden)]
    pub up_to: Option<Reference<FaceId>>,
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
    /// Whether it makes a surface (a sheet) instead of a solid: the
    /// sketch's curves swept, closed areas or not.
    #[serde(default)]
    #[arg(hidden)]
    pub surface: bool,
    /// Whether a surface's ends are closed with flat faces.
    #[serde(default)]
    #[arg(label = "Ends", options = "Open|Closed", when = "self.surface")]
    pub caps: bool,
}

impl Default for ExtrudeArgs {
    fn default() -> Self {
        Self {
            sketch: None,
            areas: Vec::new(),
            extent: Extent::Distance,
            distance: Measure::new(10.0),
            flip: false,
            up_to: None,
            combine: Combine::Join,
            target: None,
            surface: false,
            caps: false,
        }
    }
}

/// Sweeps areas of a sketch along its plane's normal into a body named
/// after the operation's id: `{id}/start` and `{id}/end` are its flat
/// ends, `{id}/side/{edge}` the face each edge of the sketch sweeps.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Extrude {
    /// The operation's id; also the name of the body it makes.
    pub id: String,
    /// What to extrude and how.
    pub args: ExtrudeArgs,
}

impl Extrude {
    /// An extrusion of `sketch` (a sketch's id), as a solid or a surface.
    pub fn of(id: impl Into<String>, sketch: Option<String>, surface: bool) -> Self {
        Self {
            id: id.into(),
            args: ExtrudeArgs {
                sketch: sketch.map(|s| Source::Profile(s).into()),
                surface,
                ..ExtrudeArgs::default()
            },
        }
    }
}

impl Operation for Extrude {
    type Args = ExtrudeArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &ExtrudeArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut ExtrudeArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = shapecraft_shape::bodies::Out::default();
        let args = &self.args;
        let distance = (*builder).measure(&args.distance, "the distance")?;
        let world = builder.tracked();
        let sketch = args.sketch.as_ref().context("pick a sketch")?.name();
        let profile = world
            .profiles
            .get(sketch)
            .with_context(|| format!("there is no sketch {sketch:?}"))?
            .clone();
        // What is swept: the outline of the areas, or, for a surface of
        // a sketch without areas, every curve.
        let regions = sweep::regions(&profile, &args.areas)?;
        anyhow::ensure!(
            args.surface || !regions.is_empty(),
            "the sketch has no closed area: only a surface can be made of it{}",
            profile.open_note()
        );
        let frame = Frame::of(&profile);
        let topo = &world.topo;
        // A cut goes into what there is: one whose areas would meet no
        // material the way it is said to go (from a sketch on a body's
        // outside), and would the other way, goes the other way. (One
        // from inside the material goes the way it is said: through
        // both ways is an extent of its own.)
        let turning = args.combine == Combine::Cut
            && matches!(
                args.extent,
                Extent::Distance | Extent::ThroughAll | Extent::ToNext
            );
        let said = frame.normal() * if args.flip { -1.0 } else { 1.0 };
        let (ahead, behind) = match turning {
            true => {
                let meets = meeting(topo, &profile, &regions, &frame);
                (meets(said), meets(-said))
            }
            false => (true, false),
        };
        let turned = turning && !ahead && behind;
        let flipped = args.flip != turned;
        let direction = frame.normal() * if flipped { -1.0 } else { 1.0 };
        let targets = if args.surface {
            Vec::new()
        } else {
            bodies::targets(topo, &self.id, &args.target)?
        };

        // How far: a distance, or beyond what it should run into.
        let positive = || {
            anyhow::ensure!(
                distance.is_finite() && distance > 0.0,
                "distance {distance} is not positive"
            );
            Ok(distance)
        };
        let beyond = |topo: &shapecraft_core::world::Topo, bodies: Vec<_>| {
            let far = bodies::reach(topo, &bodies, frame.origin, direction)
                .filter(|far| *far > 1e-9)
                .context("no body lies that way")?;
            anyhow::Ok(far * 1.1 + 1.0)
        };
        // Up to a flat face: to its plane, on past the face's own rim
        // and whichever way from the sketch that is. Where it ends, and
        // how far and which way it is swept to get past there.
        let to_plane = match args.extent {
            Extent::ToFace => args.up_to.as_ref().and_then(|face| {
                let plane =
                    Reference::<shapecraft_core::world::DatumPlane>::new(face.source.clone());
                world.resolve(&plane).ok()
            }),
            _ => None,
        };
        let to_plane = match to_plane {
            Some(plane) => {
                let from = frame.place(ui::middle(&profile, &args.areas));
                let across = frame.normal().dot(plane.normal());
                anyhow::ensure!(
                    across.abs() > 1e-9,
                    "the face stands along the way the sketch is swept: it ends nowhere on it"
                );
                let along = (plane.origin() - from).dot(plane.normal()) / across;
                anyhow::ensure!(
                    along.abs() > 1e-9,
                    "the face is where the sketch is: nothing to sweep"
                );
                let size = world.extent().map_or(1.0, |(lo, hi)| (hi - lo).length());
                Some((
                    plane,
                    from,
                    (along.abs() * 2.0 + size + 1.0) * along.signum(),
                ))
            }
            None => None,
        };
        let stop = match args.extent {
            Extent::ToFace if to_plane.is_some() => None,
            Extent::ToFace => {
                let face = args.up_to.as_ref().context("pick the face to end at")?;
                let face = world_face(topo, face)?;
                Some(bodies::body_of(topo, face).context("the face is not of a solid body")?)
            }
            _ => None,
        };
        let all_solids = || bodies::solids(topo).into_iter().map(|b| b.0).collect();
        let sign = if flipped { -1.0 } else { 1.0 };
        let range = match args.extent {
            Extent::Distance => (0.0, positive()? * sign),
            Extent::Symmetric => (-positive()? / 2.0, positive()? / 2.0),
            Extent::ThroughAll | Extent::ToNext => (0.0, beyond(topo, all_solids())? * sign),
            Extent::ThroughBoth => {
                // As far as the bodies reach, each way from the sketch.
                let all: Vec<_> = all_solids();
                let far = |way: f64| {
                    bodies::reach(topo, &all, frame.origin, frame.normal() * way)
                        .filter(|far| *far > 1e-9)
                        .map_or(0.0, |far| far * 1.1 + 1.0)
                };
                let (back, on) = (far(-1.0), far(1.0));
                anyhow::ensure!(back + on > 0.0, "no body lies either way");
                (-back, on)
            }
            Extent::ToFace => match &to_plane {
                Some((.., far)) => (0.0, *far),
                None => (0.0, beyond(topo, stop.into_iter().collect())? * sign),
            },
        };
        // How far it is swept is a size of the model.
        if matches!(args.extent, Extent::Distance | Extent::Symmetric) {
            use shapecraft_core::world::{Annotation, Measured};
            let from = frame.place(ui::middle(&profile, &args.areas));
            let measured = Measured::Length {
                from: from + frame.normal() * range.0,
                to: from + frame.normal() * range.1,
                along: None,
            };
            let held = Annotation::held(measured, distance, &args.distance, 0.0);
            out.made
                .annotations
                .push((format!("{}/distance", self.id), held));
        }
        let make = if args.surface {
            Make::Sheet { caps: args.caps }
        } else {
            Make::Solid
        };
        let trimmed = matches!(args.extent, Extent::ToNext | Extent::ToFace);
        if args.surface {
            // Every curve of the sketch, closed or left open.
            anyhow::ensure!(!trimmed, "a surface ends at a distance: trim it afterwards");
            let sheet = sweep::sheet_of(&profile, &args.areas, &|scratch, walks, closed| {
                sweep::extrude(scratch, &self.id, &profile, walks, closed, range, make)
            })?;
            out.write_sheet(&sheet, &self.id);
            return Ok(out.made);
        }
        // (Swept in a place of its own, then put with the bodies there are.)
        let walks = sweep::outline(&profile, &regions)?;
        let mut scratch = shapecraft_core::world::topo::Topo::new();
        let body = sweep::extrude(&mut scratch, &self.id, &profile, &walks, true, range, make)?;
        if args.combine == Combine::NewBody && !trimmed {
            out.take(&scratch, body)?;
            return Ok(out.made);
        }
        // As a solid for booleans: cut to where it ends, then combined.
        let mut tool = read(&scratch, body)?;
        match (args.extent, stop) {
            (Extent::ToFace, _) if to_plane.is_some() => {
                // What of it is on the sketch's side of the plane.
                let (plane, from, _) = to_plane.as_ref().expect("just asked");
                let [front, behind] =
                    shapecraft_shape::halves(&tool, plane.origin(), plane.normal())?;
                let ahead = (*from - plane.origin()).dot(plane.normal()) > 0.0;
                tool = if ahead { front } else { behind };
            }
            (Extent::ToFace, Some(stop)) => tool = bodies::up_to(topo, &self.id, &tool, &[stop])?,
            (Extent::ToNext, _) if args.combine == Combine::Cut => {
                tool = bodies::first_common(topo, &tool, &targets, frame.origin, direction)?;
            }
            (Extent::ToNext, _) => {
                let stops: Vec<_> = targets.iter().map(|t| t.0).collect();
                anyhow::ensure!(!stops.is_empty(), "there is no body to extrude up to");
                tool = bodies::up_to(topo, &self.id, &tool, &stops)?;
            }
            _ => {}
        }
        bodies::settle_into(topo, &mut out, &self.id, tool, args.combine, &targets)?;
        Ok(out.made)
    }

    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        (): &(),
    ) -> EditVisualization<Edit<ExtrudeArgs, ()>> {
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
            // An arrow from the middle of what is swept: dragging it sets
            // the distance, and through the sketch turns it around.
            let frame = Frame::of(profile);
            let from = frame.place(ui::middle(profile, &args.areas));
            let value = args
                .distance
                .resolve(context.parameters)
                .unwrap_or(args.distance.value);
            let arrow = match args.extent {
                Extent::Distance => {
                    let sign = if args.flip { -1.0 } else { 1.0 };
                    let drag = ui::set(args, |a, x: f64| {
                        // Dragged through the sketch, it goes the other
                        // way: what was joined on is cut out.
                        if x < 0.0 {
                            a.flip = !a.flip;
                            a.combine = a.combine.turned();
                        }
                        a.distance = Measure::new(x.abs());
                    });
                    LinearHandle::new(from, frame.normal() * sign, value, drag)
                }
                Extent::Symmetric => {
                    let drag = ui::set(args, |a, x: f64| a.distance = Measure::new(2.0 * x.abs()));
                    LinearHandle::new(from, frame.normal(), value / 2.0, drag)
                }
                _ => None,
            };
            if let Some(arrow) = arrow {
                handles.insert("distance".to_owned(), arrow.into());
            }
        }
        EditVisualization::new(VBox::new(fields), handles).map(Edit::args)
    }

    fn summary(&self) -> String {
        let op = self;
        let a = &op.args;
        let of = a.sketch.as_ref().map_or("(no sketch)", |s| s.name());
        let how = match a.extent {
            crate::Extent::Distance => format!("{}", a.distance),
            crate::Extent::Symmetric => format!("{} both ways", a.distance),
            crate::Extent::ThroughAll => "through all".to_owned(),
            crate::Extent::ThroughBoth => "through all, both ways".to_owned(),
            crate::Extent::ToNext => "to next".to_owned(),
            crate::Extent::ToFace => "to a face".to_owned(),
        };
        let what = if a.surface {
            "surface"
        } else {
            a.combine.label()
        };
        format!("{of}, {how}, {}", what.to_lowercase())
    }
}

/// The face `face` refers to in `topo`.
fn world_face(
    topo: &shapecraft_core::world::Topo,
    face: &Reference<FaceId>,
) -> anyhow::Result<FaceId> {
    let name = face.source.name();
    topo.resolve::<FaceId>(name)
        .with_context(|| format!("there is no face {name:?}"))
}

/// The field that picks the sketch (what was picked of another sketch
/// goes with it), and what of it is taken.
fn sketch_fields(args: &ExtrudeArgs, _: &EditContext<'_>) -> Vec<Widget<ExtrudeArgs>> {
    let set = ui::set(args, |a, sketch| {
        a.sketch = sketch;
        a.areas.clear();
    });
    ui::sketch_fields(&args.sketch, args.areas.len(), set)
}

/// How far it goes, by its extent, with the button that turns it
/// around beside it.
fn how_far(args: &ExtrudeArgs, context: &EditContext<'_>) -> Vec<Widget<ExtrudeArgs>> {
    // How far, with the button that turns it around beside it.
    let flip = || {
        ui::flip(args, |a| {
            a.flip = !a.flip;
            a.combine = a.combine.turned();
        })
    };
    let distance = || -> Widget<ExtrudeArgs> {
        NumericInput::measure(
            "Distance",
            &args.distance,
            context.parameters,
            ui::set(args, |a, d| a.distance = d),
        )
        .into()
    };
    let row: Widget<ExtrudeArgs> = match args.extent {
        Extent::Distance => HBox::new(vec![distance(), flip()]).into(),
        Extent::Symmetric => distance(),
        Extent::ThroughBoth => Label::new("Direction", "both ways", Mood::Plain).into(),
        Extent::ToFace => HBox::new(vec![
            ReferenceInput::new(
                "Up to",
                args.up_to.clone(),
                ui::set(args, |a, face| a.up_to = Some(face)),
            )
            .into(),
            flip(),
        ])
        .into(),
        Extent::ThroughAll | Extent::ToNext => {
            let way = if args.flip {
                "against the normal"
            } else {
                "along the normal"
            };
            HBox::new(vec![
                Label::new("Direction", way, Mood::Plain).into(),
                flip(),
            ])
            .into()
        }
    };
    vec![row]
}

impl shapecraft_core::Choices for Extent {
    const ALL: &'static [Self] = &Self::ALL;

    fn label(self) -> &'static str {
        Self::label(self)
    }
}

/// Whether the areas `regions` of `profile` (in `frame`), swept the way
/// asked, meet the material of the solid bodies of `topo`: looked for
/// from points spread over the areas, along the way out as far as the
/// bodies reach, more closely near the sketch.
fn meeting<'a>(
    topo: &'a shapecraft_core::world::Topo,
    profile: &'a shapecraft_core::world::Profile,
    regions: &[usize],
    frame: &'a Frame,
) -> impl Fn(Point3) -> bool + 'a {
    let all: Vec<_> = bodies::solids(topo).into_iter().map(|b| b.0).collect();
    let solids: Vec<_> = all.iter().filter_map(|b| read(topo, *b).ok()).collect();
    // Points inside the areas: of a grid over each, those its outline
    // holds and no hole of it does.
    let inside = |p: Point2, outline: &[Point2]| {
        let mut odd = false;
        for (k, a) in outline.iter().enumerate() {
            let b = outline[(k + 1) % outline.len()];
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                odd = !odd;
            }
        }
        odd
    };
    let mut points: Vec<Point3> = Vec::new();
    for &r in regions {
        let region = &profile.regions[r];
        let outer = sweep::outline_points(profile, &region.outer);
        let holes: Vec<Vec<Point2>> = region
            .holes
            .iter()
            .map(|h| sweep::outline_points(profile, h))
            .collect();
        let (Some(lo), Some(hi)) = (
            outer.iter().copied().reduce(Point2::min),
            outer.iter().copied().reduce(Point2::max),
        ) else {
            continue;
        };
        let inside_area = |p: Point2| inside(p, &outer) && !holes.iter().any(|h| inside(p, h));
        for i in 0..5 {
            for j in 0..5 {
                let p = lo + (hi - lo) * Point2::new(0.1 + 0.2 * i as f64, 0.1 + 0.2 * j as f64);
                if inside_area(p) {
                    points.push(frame.place(p));
                }
            }
        }
        // (And just inside its outline: an area that reaches into a wall
        // only at its edges — a groove — meets it there.)
        let nudge = (hi - lo).length() * 1e-3;
        for w in outer.windows(2) {
            let (a, b) = (w[0], w[1]);
            let Some(along) = (b - a).try_normalize() else {
                continue;
            };
            let middle = (a + b) / 2.0;
            for p in [a + along * nudge, middle, b - along * nudge] {
                for side in [-1.0, 1.0] {
                    let q = p + along.perp() * (nudge * side);
                    if inside_area(q) {
                        points.push(frame.place(q));
                    }
                }
            }
        }
    }
    move |way: Point3| {
        let holders: Vec<_> = solids.iter().map(shapecraft_boolean::Holder::new).collect();
        points.iter().any(|&p| {
            let Some(far) = bodies::reach(topo, &all, p, way).filter(|far| *far > 1e-9) else {
                return false;
            };
            (1..=32).any(|k| {
                let t = (f64::from(k) / 32.0).powi(2);
                let q = p + way * (far * t);
                holders.iter().any(|h| h.holds(q) == Some(true))
            })
        })
    }
}
