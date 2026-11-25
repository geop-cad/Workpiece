//! Sweep and loft: an area of a sketch carried along a path, or a solid
//! through the areas of several sketches.

use anyhow::Context;
use workpiece_boolean::{Op, Solid, combine};
use workpiece_core::Reference;
use workpiece_core::math::Point3;
use workpiece_core::widgets::ReferenceInput;
use workpiece_core::world::DatumAxis;
use workpiece_core::world::topo::Curve3;
use workpiece_core::{Builder, EditContext, Measure, Operation, ReferenceKind, Source, Widget};

use workpiece_shape::bodies::{self, Combine};
use workpiece_shape::paths::{curve, path};
use workpiece_shape::sweep::{self, Frame};
use workpiece_shape::swept::{along_path, through_sections};
use workpiece_shape::ui;

/// What is swept or lofted, and what it becomes.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct SweptLoftArgs {
    /// The sketches whose areas are the sections: one, to sweep along
    /// the path; two or more, in order, to loft through.
    #[arg(required)]
    pub sections: Vec<workpiece_core::Reference<workpiece_core::Sketched>>,
    /// The sketch whose curves are the path a section is swept along;
    /// none, for a loft.
    #[serde(default)]
    #[arg(with = "path_fields")]
    pub path: Option<workpiece_core::Reference<workpiece_core::SketchOrBody>>,
    /// The axis a section is swept around as a coil, without a path:
    /// along a helix about it, from where the section is.
    #[serde(default)]
    #[arg(label = "Coil axis", empty = "none", when = "self.path.is_none()")]
    pub coil: Option<Reference<DatumAxis>>,
    /// How far a coil advances along its axis in a turn.
    #[serde(default = "one")]
    #[arg(when = "self.coil.is_some()")]
    pub pitch: Measure,
    /// How many turns a coil makes.
    #[serde(default = "one")]
    #[arg(row, when = "self.coil.is_some()")]
    pub turns: Measure,
    /// Whether a coil is left-handed.
    #[serde(default)]
    #[arg(
        label = "Hand",
        options = "Right-handed|Left-handed",
        when = "self.coil.is_some()"
    )]
    pub left: bool,
    /// How far a swept section turns about the path on its way, in
    /// degrees.
    #[arg(when = "self.path.is_some() || self.coil.is_some()")]
    pub twist: Measure,
    /// How many times its size a swept section is at the path's end.
    #[arg(row, when = "self.path.is_some() || self.coil.is_some()")]
    pub scale: Measure,
    /// Whether it makes a surface, not a solid: what the outlines sweep,
    /// without the flat ends.
    #[serde(default)]
    #[arg(hidden)]
    pub surface: bool,
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
}

fn one() -> Measure {
    Measure::new(1.0)
}

impl Default for SweptLoftArgs {
    fn default() -> Self {
        Self {
            sections: Vec::new(),
            path: None,
            coil: None,
            pitch: one(),
            turns: Measure::new(3.0),
            left: false,
            surface: false,
            twist: Measure::new(0.0),
            scale: Measure::new(1.0),
            combine: Combine::Join,
            target: None,
        }
    }
}

/// Sweeps the areas of a sketch along the curves of another, or lofts a
/// solid through the areas of several sketches, into a body named after
/// the operation's id.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SweptLoft {
    /// The operation's id; also the name of the body it makes.
    pub id: String,
    /// What is swept or lofted, and how.
    pub args: SweptLoftArgs,
}

/// The outlines of the areas of the sketch `source` names: each its
/// sides in space, one after the other, and whether it is around a
/// hole.
fn outlines(
    world: &workpiece_core::World,
    source: &Source,
) -> anyhow::Result<Vec<(Vec<Curve3>, bool)>> {
    let name = source.name();
    let profile = world
        .profiles
        .get(name)
        .with_context(|| format!("there is no sketch {name:?}"))?;
    let regions = sweep::regions(profile, &[])?;
    anyhow::ensure!(
        !regions.is_empty(),
        "the sketch {name:?} has no closed area{}",
        profile.open_note()
    );
    let frame = Frame::of(profile);
    let mut all = Vec::new();
    for walk in sweep::outline(profile, &regions)? {
        let points = sweep::outline_points(profile, &walk);
        let area: f64 = (0..points.len())
            .map(|k| points[k].perp_dot(points[(k + 1) % points.len()]))
            .sum();
        let sides: anyhow::Result<Vec<Curve3>> =
            walk.iter().map(|&u| curve(profile, &frame, u)).collect();
        all.push((sides?, area < 0.0));
    }
    Ok(all)
}

/// The curves of the sketch `source` names as an outline that is left
/// open: one after the other from one end to the other — and two
/// straight sides after them that close it, out past the side the
/// curves do not bulge to, so that what sweeps solids can sweep it.
/// With it, how many of its sides are the sketch's own. `None` for a
/// sketch with a closed area, or whose curves are no one open run.
fn open_outline(
    world: &workpiece_core::World,
    source: &Source,
) -> anyhow::Result<Option<(Vec<Curve3>, usize)>> {
    let Some(profile) = world.profiles.get(source.name()) else {
        return Ok(None);
    };
    if !sweep::regions(profile, &[]).is_ok_and(|r| r.is_empty()) || profile.edges.is_empty() {
        return Ok(None);
    }
    let Ok((mut curves, closes, normal)) = path(world, source, Point3::ZERO) else {
        return Ok(None);
    };
    if closes {
        return Ok(None);
    }
    let own = curves.len();
    let start = curves[0].point_at(curves[0].domain().0);
    let end = curves[own - 1].point_at(curves[own - 1].domain().1);
    let chord = start - end;
    anyhow::ensure!(
        chord.length() > 0.0,
        "the open outline ends where it starts"
    );
    // Out past the chord between its ends, away from where the curves
    // themselves are: by as far as they reach, and the chord's length.
    let across = normal.cross(chord).normalize();
    let middle = (start + end) / 2.0;
    let mut reach = 0.0_f64;
    let mut lean = 0.0;
    for curve in &curves {
        let (a, b) = curve.domain();
        for k in 0..=8 {
            let off = (curve.point_at(a + (b - a) * f64::from(k) / 8.0) - middle).dot(across);
            reach = reach.max(off.abs());
            lean += off;
        }
    }
    let side = if lean > 0.0 { -1.0 } else { 1.0 };
    let apex = middle + across * (side * (reach + chord.length()));
    curves.push(Curve3::line(end, apex)?);
    curves.push(Curve3::line(apex, start)?);
    Ok(Some((curves, own)))
}

/// The outlines a sweep or a loft is of: those of the sketch's areas —
/// or, for a surface, of a sketch whose curves are left open, the one
/// outline [`open_outline`] closes for it.
fn outlines_of(
    world: &workpiece_core::World,
    source: &Source,
    surface: bool,
) -> anyhow::Result<Vec<(Vec<Curve3>, bool)>> {
    if surface && let Some((sides, _)) = open_outline(world, source)? {
        return Ok(vec![(sides, false)]);
    }
    outlines(world, source)
}

impl SweptLoft {
    /// The solid it makes.
    fn solid(
        &self,
        world: &workpiece_core::World,
        twist: f64,
        scale: f64,
        (pitch, turns): (f64, f64),
    ) -> anyhow::Result<Solid> {
        let args = &self.args;
        let first = args.sections.first().context("pick the sketch to sweep")?;
        if let Some(axis) = &args.coil {
            anyhow::ensure!(
                args.sections.len() == 1 && args.path.is_none(),
                "one sketch is coiled around an axis, without a path"
            );
            let axis = world.resolve(axis)?;
            let loops = outlines_of(world, first, args.surface)?;
            // From the middle of the section's corners.
            let corners: Vec<Point3> = loops
                .iter()
                .flat_map(|l| l.0.iter().map(|c| c.point_at(c.domain().0)))
                .collect();
            let from = corners.iter().sum::<Point3>() / corners.len() as f64;
            // Past one turn the section comes round above itself: it
            // has to fit into what one turn rises.
            let heights = loops.iter().flat_map(|l| &l.0).flat_map(|c| {
                let (a, b) = c.domain();
                (0..=64).map(move |k| c.point_at(a + (b - a) * f64::from(k) / 64.0))
            });
            let heights: Vec<f64> = heights.map(|p| p.dot(axis.direction())).collect();
            let tall = heights.iter().copied().fold(f64::MIN, f64::max)
                - heights.iter().copied().fold(f64::MAX, f64::min);
            anyhow::ensure!(
                turns <= 1.0 || tall <= pitch * (1.0 + 1e-9),
                "the turns run into each other: the sketch is {tall:.4} along the axis, and a turn rises {pitch:.4}"
            );
            let along = workpiece_shape::swept::helix(
                axis.origin(),
                axis.direction(),
                from,
                pitch,
                turns,
                0.0,
                args.left,
            )?;
            let mut whole: Option<Solid> = None;
            for holes in [false, true] {
                for (k, (sides, hole)) in loops.iter().enumerate() {
                    if *hole != holes {
                        continue;
                    }
                    let id = format!("{}/l{k}", self.id);
                    let up = axis.direction();
                    let swept = along_path(
                        &id,
                        sides,
                        std::slice::from_ref(&along),
                        false,
                        up,
                        twist,
                        scale,
                    )?;
                    whole = Some(match whole {
                        None => swept,
                        Some(sofar) => {
                            let op = if holes { Op::Cut } else { Op::Union };
                            combine(op, &sofar, &swept)
                                .context("putting the coiled outlines together")?
                                .solid
                        }
                    });
                }
            }
            return whole.context("nothing to coil");
        }
        let Some(along) = &args.path else {
            anyhow::ensure!(
                args.sections.len() >= 2,
                "pick a path to sweep along, or more sketches to loft through"
            );
            let mut sections = Vec::new();
            for source in &args.sections {
                let mut loops = outlines_of(world, source, args.surface)?;
                anyhow::ensure!(
                    loops.len() == 1,
                    "the sketch {:?} has several outlines: a loft goes through one of each",
                    source.name()
                );
                sections.push(loops.remove(0).0);
            }
            return through_sections(&self.id, &sections);
        };
        anyhow::ensure!(
            args.sections.len() == 1,
            "one sketch is swept along a path: sketches are lofted through without one"
        );
        let loops = outlines_of(world, first, args.surface)?;
        let start = |sides: &[Curve3]| sides[0].point_at(sides[0].domain().0);
        let (curves, closes, up) = path(world, along, start(&loops[0].0))?;
        // What the outlines around the areas sweep, less what those
        // around their holes do.
        workpiece_shape::swept::area_along_path(&self.id, &loops, &curves, closes, up, twist, scale)
    }
}

impl Operation for SweptLoft {
    type Args = SweptLoftArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &SweptLoftArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut SweptLoftArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = workpiece_shape::bodies::Out::default();
        let args = &self.args;
        let twist = (*builder).measure(&args.twist, "the twist")?.to_radians();
        let scale = (*builder).measure(&args.scale, "the scale")?;
        let pitch = (*builder).measure(&args.pitch, "the pitch")?;
        let turns = (*builder).measure(&args.turns, "the turns")?;
        let world = builder.tracked();
        // A loft through sketches whose curves are left open is a
        // surface through them as they are.
        if args.surface && args.path.is_none() && args.coil.is_none() {
            let mut runs = Vec::new();
            for source in &args.sections {
                let open = world.profiles.get(source.name()).is_some_and(|profile| {
                    sweep::regions(profile, &[]).is_ok_and(|r| r.is_empty())
                });
                match path(world, source, Point3::ZERO) {
                    Ok((curves, false, normal)) if open => runs.push((curves, normal)),
                    _ => break,
                }
            }
            if runs.len() == args.sections.len() && runs.len() >= 2 {
                let sheet = workpiece_shape::swept::through_open_sections(&self.id, &runs)?;
                out.write_sheet(&sheet, &self.id);
                return Ok(out.made);
            }
        }
        let tool = self.solid(world, twist, scale, (pitch, turns))?;
        if args.surface {
            // All of it but its flat ends, as a surface — and, of an
            // outline that was left open, but the sides that closed it.
            let own = match args.sections.first() {
                Some(first) => open_outline(world, first)?.map(|(_, own)| own),
                None => None,
            };

            let sides: Vec<usize> = (0..tool.faces.len())
                .filter(|&f| {
                    let name = tool.faces[f].name.as_deref().unwrap_or("");
                    let side = name
                        .rsplit_once("/side/e")
                        .and_then(|(_, k)| k.parse::<usize>().ok());
                    !(name.ends_with("/start") || name.ends_with("/end"))
                        && !own.zip(side).is_some_and(|(own, k)| k >= own)
                })
                .collect();
            let sheet = workpiece_shape::surface::faces_of(&tool, &sides);
            out.write_sheet(&sheet, &self.id);
            return Ok(out.made);
        }
        let topo = &world.topo;
        let targets = bodies::targets(topo, &self.id, &args.target)?;
        bodies::settle_into(topo, &mut out, &self.id, tool, args.combine, &targets)?;
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let names: Vec<&str> = self.args.sections.iter().map(|r| r.name()).collect();
        if self.args.coil.is_some() {
            return format!("{} coiled, {} turns", names.join(", "), self.args.turns);
        }
        match &self.args.path {
            Some(path) => format!("{} along {}", names.join(", "), path.name()),
            None => format!("through {}", names.join(", ")),
        }
    }
}

/// The fields that pick what the sections are swept along: the curves
/// of a sketch, or a curve in space; neither, to loft through them.
fn path_fields(args: &SweptLoftArgs, _: &EditContext<'_>) -> Vec<Widget<SweptLoftArgs>> {
    vec![
        ReferenceInput::new(
            "Path",
            args.path.clone(),
            ui::set(args, |a, path| a.path = Some(path)),
        )
        .optional("none (loft through the sections)", {
            let clear = ui::set(args, |a, (): ()| a.path = None);
            move || clear(())
        })
        .into(),
        // (The same path, picked as a curve in space.)
        ReferenceInput::of_kind(
            "or along a curve",
            ReferenceKind::Body,
            args.path
                .as_ref()
                .map(|p| p.source.clone())
                .filter(|p| matches!(p, Source::Topology(_))),
            ui::set(args, |a, path: Source| a.path = Some(path.into())),
        )
        .optional("no curve body", {
            let clear = ui::set(args, |a, (): ()| a.path = None);
            move || clear(())
        })
        .into(),
    ]
}
