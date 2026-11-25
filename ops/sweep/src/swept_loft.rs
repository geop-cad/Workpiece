//! Sweep and loft: an area of a sketch carried along a path, or a solid
//! through the areas of several sketches.

use anyhow::Context;
use workpiece_boolean::model::Walk;
use workpiece_boolean::{Op, Solid, combine};
use workpiece_core::math::Point3;
use workpiece_core::widgets::{Choice, Label, Mood, ReferenceInput, ReferenceList};
use workpiece_core::world::DatumAxis;
use workpiece_core::world::topo::{Curve3, FaceId};
use workpiece_core::{
    Anything, Builder, EditContext, Measure, Operation, Reference, ReferenceKind, SketchOrBody,
    Source, Widget,
};

use workpiece_shape::bodies::{self, Combine};
use workpiece_shape::paths::{curve, path};
use workpiece_shape::sweep::{self, Frame};
use workpiece_shape::swept::{Held, Shaping, along_path, through_sections};

/// How a loft leaves its first section or arrives at its last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SectionEnd {
    /// As the sections lie.
    #[default]
    Free,
    /// Square to the section's plane (a sketch's, a face's).
    Normal,
    /// Running on from a face smoothly (G1): tangent to it, across the
    /// section.
    Tangent,
    /// Tangent, and bending across the section as the face does (G2).
    Curvature,
}

impl workpiece_core::arguments::Choices for SectionEnd {
    const ALL: &'static [Self] = &[Self::Free, Self::Normal, Self::Tangent, Self::Curvature];

    fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Normal => "Normal to section",
            Self::Tangent => "Tangent to face (G1)",
            Self::Curvature => "Curvature to face (G2)",
        }
    }

    fn icon(self) -> Option<&'static str> {
        // (Drawn by the surface operations, which share them.)
        Some(match self {
            Self::Free => "end-free",
            Self::Normal => "end-normal",
            Self::Tangent => "end-tangent",
            Self::Curvature => "end-curvature",
        })
    }
}

/// A section of a sweep or a loft.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct Section {
    /// A sketch (its area; for a surface, its curves as they are, open
    /// too), a face (its outline), or an edge or a curve (for a
    /// surface).
    pub of: Reference<Anything>,
    /// How a loft leaves it (the first) or arrives at it (the last).
    #[serde(default)]
    #[arg(hidden)]
    pub end: SectionEnd,
    /// The face a loft runs on from there (tangent, curvature): none
    /// for the face the section is.
    #[serde(default)]
    #[arg(hidden)]
    pub face: Option<Reference<FaceId>>,
    /// Whether a loft leaves it the other way: out of the loft, back
    /// over the face (bending as the face does all the same).
    #[serde(default)]
    #[arg(hidden)]
    pub flip: bool,
    /// How full a loft leaves it: the length of the way it leaves, as a
    /// share of its natural one (1).
    #[serde(default = "natural")]
    #[arg(hidden)]
    pub magnitude: f64,
    /// Running on from a face: the way it leaves within the face's
    /// tangent plane, in degrees from square across the section.
    #[serde(default)]
    #[arg(hidden)]
    pub angle: f64,
}

fn natural() -> f64 {
    1.0
}

impl Section {
    /// A section of `source`, free at its end.
    pub fn of(source: impl Into<Source>) -> Self {
        Self {
            of: Reference::new(source.into()),
            end: SectionEnd::Free,
            face: None,
            flip: false,
            magnitude: 1.0,
            angle: 0.0,
        }
    }

    /// How a loft is held at it, the way and bend at a place given by
    /// `at`.
    fn held<'a>(&self, at: &'a dyn Fn(Point3) -> (Point3, Option<Point3>)) -> Held<'a> {
        Held {
            at,
            flip: self.flip,
            length: self.magnitude,
        }
    }
}

/// What a curve does for a sweep or a loft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Role {
    /// The path: the one section is carried along it (a sweep). One
    /// at most.
    #[default]
    Path,
    /// A guide rail: a loft through two sections or more runs along it,
    /// from the first section to the last (it touches each).
    Guide,
}

impl workpiece_core::arguments::Choices for Role {
    const ALL: &'static [Self] = &[Self::Path, Self::Guide];

    fn label(self) -> &'static str {
        match self {
            Self::Path => "Path (sweep along)",
            Self::Guide => "Guide (loft follows)",
        }
    }

    fn icon(self) -> Option<&'static str> {
        Some(match self {
            Self::Path => "rail-path",
            Self::Guide => "rail-guide",
        })
    }
}

/// A curve a sweep or a loft goes along, and what it does.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct Rail {
    /// The curves of a sketch, or a curve body.
    pub of: Reference<SketchOrBody>,
    /// What it does.
    #[serde(default)]
    #[arg(hidden)]
    pub role: Role,
}

/// What is swept or lofted, and what it becomes.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct SweptLoftArgs {
    /// The sections: one, to sweep along the path or coil; two or more,
    /// in order, to loft through. Each with how a loft leaves or
    /// arrives at it.
    #[arg(nested, with = "sections_field", group = "Sections")]
    pub sections: Vec<Section>,
    /// The curves it goes along: the path a single section is swept
    /// along, and guide rails a loft follows.
    #[serde(default)]
    #[arg(nested, with = "rails_field", group = "Along")]
    pub rails: Vec<Rail>,
    /// The axis a section is swept around as a coil, without a path:
    /// along a helix about it, from where the section is.
    #[serde(default)]
    #[arg(
        label = "Coil axis",
        empty = "none",
        group = "Coil",
        when = "self.rails.is_empty() && self.sections.len() <= 1"
    )]
    pub coil: Option<Reference<DatumAxis>>,
    /// How far a coil advances along its axis in a turn.
    #[serde(default = "one")]
    #[arg(group = "Coil", when = "self.coil.is_some()")]
    pub pitch: Measure,
    /// How many turns a coil makes.
    #[serde(default = "one")]
    #[arg(row, group = "Coil", when = "self.coil.is_some()")]
    pub turns: Measure,
    /// Whether a coil is left-handed.
    #[serde(default)]
    #[arg(
        label = "Hand",
        options = "Right-handed|Left-handed",
        icons = "hand-right|hand-left",
        group = "Coil",
        when = "self.coil.is_some()"
    )]
    pub left: bool,
    /// How far a swept section turns about the path on its way, in
    /// degrees.
    #[arg(group = "Swept", when = "self.path().is_some() || self.coil.is_some()")]
    pub twist: Measure,
    /// How many times its size a swept section is at the path's end.
    #[arg(
        row,
        group = "Swept",
        when = "self.path().is_some() || self.coil.is_some()"
    )]
    pub scale: Measure,
    /// Whether it makes a surface, not a solid: what the outlines sweep,
    /// without the flat ends.
    #[serde(default)]
    #[arg(hidden)]
    pub surface: bool,
    /// What the solid does to the bodies there are.
    #[serde(default)]
    #[arg(choice, label = "Result", group = "Result", when = "!self.surface")]
    pub combine: Combine,
    /// The body it is combined with; `None` for every solid body.
    #[serde(default)]
    #[arg(
        label = "With",
        empty = "All bodies",
        group = "Result",
        when = "!self.surface && self.combine != Combine::NewBody"
    )]
    pub target: Option<workpiece_core::Reference<workpiece_core::world::topo::BodyId>>,
}

impl SweptLoftArgs {
    /// The path, if a curve is one.
    pub fn path(&self) -> Option<&Reference<SketchOrBody>> {
        self.rails
            .iter()
            .find(|r| r.role == Role::Path)
            .map(|r| &r.of)
    }

    /// The guides.
    fn guides(&self) -> impl Iterator<Item = &Reference<SketchOrBody>> {
        self.rails
            .iter()
            .filter(|r| r.role == Role::Guide)
            .map(|r| &r.of)
    }
}

fn one() -> Measure {
    Measure::new(1.0)
}

impl Default for SweptLoftArgs {
    fn default() -> Self {
        Self {
            sections: Vec::new(),
            rails: Vec::new(),
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

/// A section as curves in space: around an area (closed, one outline;
/// the sketch's own [`outlines`] where it has holes), or left open, with
/// the normal of the plane it is in.
enum Curves {
    /// Outlines, each its sides and whether it is around a hole.
    Closed(Vec<(Vec<Curve3>, bool)>),
    /// One run of curves left open, and the normal of its plane.
    Open(Vec<Curve3>, Point3),
}

/// The face called `name`, read with the body it is of, and which face.
fn face_of(world: &workpiece_core::World, name: &str) -> anyhow::Result<(Solid, usize)> {
    workpiece_shape::on::face(&world.topo, name)
}

/// The outline of face `f` of `solid` (its first loop) as curves, one
/// after the other the way it is walked.
fn face_outline(solid: &Solid, f: usize) -> anyhow::Result<Vec<Curve3>> {
    let walk = solid.faces[f]
        .loops
        .first()
        .context("a face without an outline")?;
    let mut sides = Vec::new();
    for step in walk {
        if let Walk::Edge { edge, reversed } = step.walk {
            let edge = &solid.edges[edge];
            let piece = edge.curve.trimmed(edge.range.0, edge.range.1)?;
            sides.push(if reversed { piece.reversed() } else { piece });
        }
    }
    anyhow::ensure!(!sides.is_empty(), "a face without an outline");
    Ok(sides)
}

/// The normal of the plane curves lie in, about: of the polygon through
/// points along them.
fn plane_normal(curves: &[Curve3]) -> Point3 {
    let points: Vec<Point3> = curves
        .iter()
        .flat_map(|c| {
            let (a, b) = c.domain();
            (0..16).map(move |k| c.point_at(a + (b - a) * f64::from(k) / 16.0))
        })
        .collect();
    let centre = points.iter().sum::<Point3>() / points.len().max(1) as f64;
    let turn: Point3 = (0..points.len())
        .map(|k| (points[k] - centre).cross(points[(k + 1) % points.len()] - centre))
        .sum();
    turn.normalize_or_zero()
}

/// The section `of` as curves (see [`Curves`]); `surface`, for a sweep
/// or a loft that makes a surface: a sketch's curves left open, an
/// edge or a curve, are taken as they are.
fn section_curves(
    world: &workpiece_core::World,
    of: &Source,
    surface: bool,
) -> anyhow::Result<Curves> {
    match of {
        Source::Profile(_) => {
            if surface
                && let Some(profile) = world.profiles.get(of.name())
                && sweep::regions(profile, &[]).is_ok_and(|r| r.is_empty())
                && let Ok((curves, false, normal)) = path(world, of, Point3::ZERO)
            {
                return Ok(Curves::Open(curves, normal));
            }
            Ok(Curves::Closed(outlines(world, of)?))
        }
        Source::Topology(name) if world.topo.resolve::<FaceId>(name).is_some() => {
            let (solid, f) = face_of(world, name)?;
            Ok(Curves::Closed(vec![(face_outline(&solid, f)?, false)]))
        }
        Source::Topology(name) => {
            // An edge, or a curve body: its curves, end to end.
            let curves = match world
                .topo
                .resolve::<workpiece_core::world::topo::EdgeId>(name)
                .and_then(|e| world.topo.edge(e))
            {
                Some(edge) => {
                    let curve = world
                        .topo
                        .curve(edge.curve)
                        .context("an edge without a curve")?;
                    vec![curve.trimmed(edge.t0, edge.t1)?]
                }
                None => path(world, of, Point3::ZERO)?.0,
            };
            let first = curves[0].point_at(curves[0].domain().0);
            let last = curves[curves.len() - 1].point_at(curves[curves.len() - 1].domain().1);
            let size = curves
                .iter()
                .map(|c| c.point_at(c.domain().0).distance(first))
                .fold(first.distance(last), f64::max);
            if first.distance(last) <= size * 1e-7 {
                return Ok(Curves::Closed(vec![(curves, false)]));
            }
            anyhow::ensure!(
                surface,
                "{name:?} is no closed outline: a solid is lofted through areas (a surface through curves)"
            );
            let normal = plane_normal(&curves);
            Ok(Curves::Open(curves, normal))
        }
        _ => anyhow::bail!(
            "{} is no section: pick a sketch, a face or a curve",
            of.name()
        ),
    }
}

/// At a place of a section, the way a loft leaves it and its bend.
type HeldAt<'a> = Box<dyn Fn(Point3) -> (Point3, Option<Point3>) + 'a>;

/// How the loft is held at the section `section` (its curves `curves`),
/// in `world`.
fn held_at<'a>(
    world: &'a workpiece_core::World,
    section: &Section,
    curves: &'a [Curve3],
    normal: Point3,
) -> anyhow::Result<Option<HeldAt<'a>>> {
    Ok(match section.end {
        SectionEnd::Free => None,
        SectionEnd::Normal => {
            anyhow::ensure!(
                normal.length() > 0.0,
                "the section {} lies in no plane to be square to",
                section.of.name()
            );
            Some(Box::new(move |_| (normal, None)))
        }
        SectionEnd::Tangent | SectionEnd::Curvature => {
            let name = match &section.face {
                Some(face) => face.name().to_owned(),
                None => {
                    let own = section.of.name();
                    anyhow::ensure!(
                        world.topo.resolve::<FaceId>(own).is_some(),
                        "pick the face the loft runs on from at {own:?}"
                    );
                    own.to_owned()
                }
            };
            let (solid, f) = face_of(world, &name)?;
            let surface = solid.faces[f].surface.clone();
            let bends = section.end == SectionEnd::Curvature;
            let angle = section.angle.to_radians();
            Some(Box::new(move |p: Point3| {
                // The way along the section here, and the face's normal.
                let along = curves
                    .iter()
                    .flat_map(|c| {
                        let (a, b) = c.domain();
                        (0..=32).map(move |k| {
                            let t = a + (b - a) * f64::from(k) / 32.0;
                            let (q, d) = c.derivative_at(t);
                            (q.distance(p), d)
                        })
                    })
                    .min_by(|x, y| x.0.total_cmp(&y.0))
                    .map_or(Point3::X, |(_, d)| d);
                let (uv, _) = nearest_uv(&surface, p);
                let (_, du, dv) = surface.derivatives_at(uv.x, uv.y);
                let n = du.cross(dv).normalize_or_zero();
                // (Square across the section in the face, turned within
                // it as asked.)
                let square = n.cross(along).normalize_or_zero();
                let way = square * angle.cos() + n.cross(square) * angle.sin();
                let bend = bends.then(|| n * bend_along(&surface, uv, way));
                (way, bend)
            }))
        }
    })
}

/// The (u, v) of `surface` nearest `p`, and how far that is.
fn nearest_uv(
    surface: &workpiece_core::world::topo::NurbsSurface,
    p: Point3,
) -> (workpiece_core::math::Point2, f64) {
    use workpiece_core::math::Point2;
    let ((u0, u1), (v0, v1)) = surface.domain();
    let mut best = (f64::INFINITY, Point2::new(u0, v0));
    for i in 0..=16 {
        for j in 0..=16 {
            let uv = Point2::new(
                u0 + (u1 - u0) * f64::from(i) / 16.0,
                v0 + (v1 - v0) * f64::from(j) / 16.0,
            );
            let far = surface.point_at(uv.x, uv.y).distance(p);
            if far < best.0 {
                best = (far, uv);
            }
        }
    }
    let (uv, far) = workpiece_boolean::geometry::foot(surface, p, best.1);
    (uv, far)
}

/// How much `surface` bends at `uv` along the direction `way` (in
/// space, along it): its normal curvature that way.
fn bend_along(
    surface: &workpiece_core::world::topo::NurbsSurface,
    uv: workpiece_core::math::Point2,
    way: Point3,
) -> f64 {
    let (_, du, dv) = surface.derivatives_at(uv.x, uv.y);
    let normal = du.cross(dv).normalize_or_zero();
    let (a, b, c) = (du.dot(du), du.dot(dv), dv.dot(dv));
    let det = a * c - b * b;
    if det.abs() < 1e-300 {
        return 0.0;
    }
    let (r, s) = (way.dot(du), way.dot(dv));
    let step = workpiece_core::math::Point2::new((r * c - s * b) / det, (s * a - r * b) / det);
    let ((u0, u1), (v0, v1)) = surface.domain();
    let e = 1e-4 * ((u1 - u0).abs() + (v1 - v0).abs()) / (step.length().max(1e-300) * 2.0);
    let at = |k: f64| {
        let q = uv + step * (e * k);
        surface.point_at(q.x.clamp(u0, u1), q.y.clamp(v0, v1))
    };
    // (Centred where it can be; one-sided at the surface's edge.)
    let second = (at(1.0) - at(0.0) * 2.0 + at(-1.0)) / (e * e);
    second.dot(normal)
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
        let first = args.sections.first().context("pick the section to sweep")?;
        let first = &first.of.source;
        anyhow::ensure!(
            args.rails.iter().filter(|r| r.role == Role::Path).count() <= 1,
            "a sweep goes along one path: make the other curves guides"
        );
        if let Some(axis) = &args.coil {
            anyhow::ensure!(
                args.sections.len() == 1 && args.rails.is_empty(),
                "one section is coiled around an axis, without a path"
            );
            let axis = world.resolve(axis)?;
            let loops = closed_loops(world, first, args.surface)?;
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
                "the turns run into each other: the section is {tall:.4} along the axis, and a turn rises {pitch:.4}"
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
        let Some(along) = args.path() else {
            anyhow::ensure!(
                args.sections.len() >= 2,
                "pick a path to sweep along, or more sections to loft through"
            );
            let mut sections = Vec::new();
            let mut normals = Vec::new();
            for section in &args.sections {
                let mut loops = closed_loops(world, &section.of.source, args.surface)?;
                anyhow::ensure!(
                    loops.len() == 1,
                    "the section {:?} has several outlines: a loft goes through one of each",
                    section.of.name()
                );
                let sides = loops.remove(0).0;
                normals.push(plane_normal(&sides));
                sections.push(sides);
            }
            let guides = self.guide_curves(world, &sections)?;
            let last = sections.len() - 1;
            let start = held_at(world, &args.sections[0], &sections[0], normals[0])?;
            let end = held_at(world, &args.sections[last], &sections[last], normals[last])?;
            let how = Shaping {
                ends: [
                    start.as_deref().map(|at| args.sections[0].held(at)),
                    end.as_deref().map(|at| args.sections[last].held(at)),
                ],
                guides,
            };
            return through_sections(&self.id, &sections, &how);
        };
        anyhow::ensure!(
            args.sections.len() == 1,
            "one section is swept along a path: sections are lofted through without one (a curve that shapes a loft is a guide)"
        );
        anyhow::ensure!(
            args.guides().next().is_none(),
            "guides shape a loft through two sections or more, not a sweep"
        );
        let loops = closed_loops(world, first, args.surface)?;
        let start = |sides: &[Curve3]| sides[0].point_at(sides[0].domain().0);
        let (curves, closes, up) = path(world, &along.source, start(&loops[0].0))?;
        // What the outlines around the areas sweep, less what those
        // around their holes do.
        workpiece_shape::swept::area_along_path(&self.id, &loops, &curves, closes, up, twist, scale)
    }

    /// The guides' curves, each from the first of `sections` to the
    /// last (turned around if it was drawn the other way).
    fn guide_curves(
        &self,
        world: &workpiece_core::World,
        sections: &[Vec<Curve3>],
    ) -> anyhow::Result<Vec<Vec<Curve3>>> {
        let near = |curves: &[Curve3], p: Point3| {
            curves
                .iter()
                .flat_map(|c| c.polyline(c.domain().0, c.domain().1))
                .map(|q| q.distance(p))
                .fold(f64::INFINITY, f64::min)
        };
        let mut out = Vec::new();
        for guide in self.args.guides() {
            let start = sections[0][0].point_at(sections[0][0].domain().0);
            let (mut curves, _, _) = path(world, &guide.source, start)?;
            let first = curves[0].point_at(curves[0].domain().0);
            let last = curves[curves.len() - 1].point_at(curves[curves.len() - 1].domain().1);
            if near(&sections[0], last) < near(&sections[0], first) {
                curves.reverse();
                for c in &mut curves {
                    *c = c.reversed();
                }
            }
            out.push(curves);
        }
        Ok(out)
    }
}

/// The closed outlines of the section `of`: a sketch's areas, a face's
/// outline; for a surface, a sketch whose curves are left open closed
/// as [`open_outline`] closes it.
fn closed_loops(
    world: &workpiece_core::World,
    of: &Source,
    surface: bool,
) -> anyhow::Result<Vec<(Vec<Curve3>, bool)>> {
    match of {
        Source::Profile(_) => outlines_of(world, of, surface),
        _ => match section_curves(world, of, false)? {
            Curves::Closed(loops) => Ok(loops),
            Curves::Open(..) => anyhow::bail!("{} is no closed outline", of.name()),
        },
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
        // A loft through sections that are left open is a surface
        // through them as they are.
        if args.surface && args.path().is_none() && args.coil.is_none() && args.sections.len() >= 2
        {
            let mut runs = Vec::new();
            for section in &args.sections {
                match section_curves(world, &section.of.source, true)? {
                    Curves::Open(curves, normal) => runs.push((curves, normal)),
                    Curves::Closed(_) => break,
                }
            }
            if runs.len() == args.sections.len() {
                // A straight run lies in no one plane: in that through
                // it square to the way to the run beside it.
                let middle = |run: &[Curve3]| {
                    let points: Vec<Point3> = run
                        .iter()
                        .flat_map(|c| [c.point_at(c.domain().0), c.point_at(c.domain().1)])
                        .collect();
                    points.iter().sum::<Point3>() / points.len() as f64
                };
                let middles: Vec<Point3> = runs.iter().map(|r| middle(&r.0)).collect();
                for j in 0..runs.len() {
                    if runs[j].1.length() > 0.5 {
                        continue;
                    }
                    let run = &runs[j].0;
                    let chord = run[run.len() - 1].point_at(run[run.len() - 1].domain().1)
                        - run[0].point_at(run[0].domain().0);
                    let other = middles[if j == 0 { 1 } else { j - 1 }];
                    let onward = other - middles[j];
                    let chord = chord.normalize_or_zero();
                    runs[j].1 = (onward - chord * onward.dot(chord)).normalize_or_zero();
                }
                let curves: Vec<Vec<Curve3>> = runs.iter().map(|r| r.0.clone()).collect();
                let guides = self.guide_curves(world, &curves)?;
                let last = runs.len() - 1;
                let start = held_at(world, &args.sections[0], &runs[0].0, runs[0].1)?;
                let end = held_at(world, &args.sections[last], &runs[last].0, runs[last].1)?;
                let how = Shaping {
                    ends: [
                        start.as_deref().map(|at| args.sections[0].held(at)),
                        end.as_deref().map(|at| args.sections[last].held(at)),
                    ],
                    guides,
                };
                let sheet = workpiece_shape::swept::through_open_sections(&self.id, &runs, &how)?;
                out.write_sheet(&sheet, &self.id);
                return Ok(out.made);
            }
        }
        let tool = self.solid(world, twist, scale, (pitch, turns))?;
        if args.surface {
            // All of it but its flat ends, as a surface — and, of an
            // outline that was left open, but the sides that closed it.
            let own = match args.sections.first() {
                Some(first) => open_outline(world, &first.of.source)?.map(|(_, own)| own),
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

    fn visualize_edit(
        &self,
        context: &workpiece_core::EditContext<'_>,
        (): &(),
    ) -> workpiece_core::EditVisualization<workpiece_core::Edit<SweptLoftArgs, ()>> {
        let args = &self.args;
        let fields = workpiece_core::Arguments::fields(args, context);
        let mut handles = std::collections::BTreeMap::new();
        let n = args.sections.len();
        if n >= 2 && args.path().is_none() && args.coil.is_none() {
            for (k, other) in [(0, n - 1), (n - 1, 0)] {
                if let Some(handle) = leaving_handle(context.world, args, k, other) {
                    handles.insert(format!("section{k}"), handle);
                }
            }
        }
        workpiece_core::EditVisualization::new(workpiece_core::widgets::VBox::new(fields), handles)
            .map(workpiece_core::Edit::args)
    }

    fn summary(&self) -> String {
        let names: Vec<&str> = self.args.sections.iter().map(|s| s.of.name()).collect();
        if self.args.coil.is_some() {
            return format!("{} coiled, {} turns", names.join(", "), self.args.turns);
        }
        match self.args.path() {
            Some(path) => format!("{} along {}", names.join(", "), path.name()),
            None => {
                let guides = self.args.guides().count();
                let along = match guides {
                    0 => String::new(),
                    1 => ", 1 guide".to_owned(),
                    n => format!(", {n} guides"),
                };
                format!("through {}{along}", names.join(", "))
            }
        }
    }
}

/// The last part of a name, what is told apart in a list, cut short
/// to fit beside a caption.
fn short(name: &str) -> String {
    let parts: Vec<&str> = name.rsplit('/').take(2).collect();
    let said = parts.into_iter().rev().collect::<Vec<_>>().join("/");
    if said.chars().count() > 22 {
        let cut: String = said.chars().take(21).collect();
        format!("{cut}…")
    } else {
        said
    }
}

/// The box of the sections, in order, and how a loft leaves the first
/// and arrives at the last of them.
fn sections_field(args: &SweptLoftArgs, _: &EditContext<'_>) -> Vec<Widget<SweptLoftArgs>> {
    let picked: Vec<Source> = args.sections.iter().map(|s| s.of.source.clone()).collect();
    let was = args.clone();
    let set = move |picked: Vec<Source>| {
        let mut new = was.clone();
        new.sections = picked
            .into_iter()
            .map(|source| {
                was.sections
                    .iter()
                    .find(|s| s.of.source == source)
                    .cloned()
                    .unwrap_or_else(|| Section::of(source))
            })
            .collect();
        new
    };
    let kinds = if args.surface {
        vec![
            ReferenceKind::Profile,
            ReferenceKind::Face,
            ReferenceKind::Edge,
        ]
    } else {
        vec![ReferenceKind::Profile, ReferenceKind::Face]
    };
    let mut out: Vec<Widget<SweptLoftArgs>> = vec![
        ReferenceList::new("In order", kinds, picked, set)
            .with_icon("f-sketch")
            .required()
            .into(),
    ];
    let n = args.sections.len();
    // A loft's ends: how it leaves the first and arrives at the last.
    if n >= 2 && args.path().is_none() {
        use workpiece_core::arguments::Choices;
        for (k, said) in [(0, "Start"), (n - 1, "End")] {
            let section = &args.sections[k];
            let (now, all) = (args.clone(), SectionEnd::ALL);
            let choice = Choice::new(
                format!("{said}: {}", short(section.of.name())),
                all.iter().map(|c| c.label().to_owned()).collect(),
                all.iter().position(|c| *c == section.end).unwrap_or(0),
                move |i| {
                    let mut new = now.clone();
                    new.sections[k].end = all[i.min(all.len() - 1)];
                    new
                },
            )
            .with_icons(all.iter().map(|c| c.icon()).collect())
            .with_icon("f-extent")
            .drop_down();
            out.push(choice.into());
            if matches!(section.end, SectionEnd::Tangent | SectionEnd::Curvature) {
                let (set, clear) = (args.clone(), args.clone());
                let own = matches!(&section.of.source, Source::Topology(_));
                let face = ReferenceInput::of_kind(
                    "From face",
                    ReferenceKind::Face,
                    section.face.as_ref().map(|f| f.source.clone()),
                    move |source: Source| {
                        let mut new = set.clone();
                        new.sections[k].face = Some(source.into());
                        new
                    },
                )
                .optional(
                    if own {
                        "The section's own face"
                    } else {
                        "Pick the face"
                    },
                    move || {
                        let mut new = clear.clone();
                        new.sections[k].face = None;
                        new
                    },
                );
                out.push(face.into());
            }
            if section.end != SectionEnd::Free {
                out.push(leaving_row(args, k));
            }
        }
    }
    out
}

/// The curves of section `k`, and the middle of them.
fn section_middle(
    world: &workpiece_core::World,
    args: &SweptLoftArgs,
    k: usize,
) -> Option<(Vec<Curve3>, Point3)> {
    let curves = match section_curves(world, &args.sections[k].of.source, args.surface).ok()? {
        Curves::Open(curves, _) => curves,
        Curves::Closed(loops) => loops.into_iter().next()?.0,
    };
    let points: Vec<Point3> = curves
        .iter()
        .flat_map(|c| {
            let (a, b) = c.domain();
            (0..8).map(move |i| c.point_at(a + (b - a) * f64::from(i) / 8.0))
        })
        .collect();
    let middle = points.iter().sum::<Point3>() / points.len().max(1) as f64;
    Some((curves, middle))
}

/// The handle of how a loft leaves (or arrives at) section `k` running
/// on from a face: a knob at the tip of the way it leaves, at a place of
/// the section, moved in the face's tangent plane — turning it within
/// the face, and pulling it out or in (through to the other side of the
/// section, it leaves the other way). `other` is the section at the
/// loft's other end: the way into the loft.
fn leaving_handle(
    world: &workpiece_core::World,
    args: &SweptLoftArgs,
    k: usize,
    other: usize,
) -> Option<workpiece_core::handles::Handle<SweptLoftArgs>> {
    let section = &args.sections[k];
    if !matches!(section.end, SectionEnd::Tangent | SectionEnd::Curvature) {
        return None;
    }
    let (curves, middle) = section_middle(world, args, k)?;
    let (_, there) = section_middle(world, args, other)?;
    let name = match &section.face {
        Some(face) => face.name().to_owned(),
        None => section.of.name().to_owned(),
    };
    let (solid, f) = face_of(world, &name).ok()?;
    let surface = &solid.faces[f].surface;
    // The place of the section nearest its middle, and the way along it.
    let (at, along) = curves
        .iter()
        .flat_map(|c| {
            let (a, b) = c.domain();
            (0..=32).map(move |i| c.derivative_at(a + (b - a) * f64::from(i) / 32.0))
        })
        .min_by(|x, y| x.0.distance(middle).total_cmp(&y.0.distance(middle)))?;
    let (uv, _) = nearest_uv(surface, at);
    let (_, du, dv) = surface.derivatives_at(uv.x, uv.y);
    let normal = du.cross(dv).try_normalize()?;
    let onward = if k == 0 {
        there - middle
    } else {
        middle - there
    };
    let square = normal.cross(along).try_normalize()?;
    let square = if square.dot(onward) < 0.0 {
        -square
    } else {
        square
    };
    let a = section.angle.to_radians();
    let way = square * a.cos() + normal.cross(square) * a.sin();
    let way = if way.dot(onward) < 0.0 { -way } else { way };
    let way = if section.flip { -way } else { way };
    let reach = middle.distance(there).max(1e-9) / 4.0;
    let tip = at + way * (section.magnitude * reach);
    let was = args.clone();
    let handle = workpiece_core::handles::PlanarHandle::new(tip, normal, move |p| {
        let v = p - at;
        let v = v - normal * v.dot(normal);
        let mut new = was.clone();
        let section = &mut new.sections[k];
        section.magnitude = (v.length() / reach).max(1e-3);
        let angle = normal
            .cross(square)
            .dot(v)
            .atan2(square.dot(v))
            .to_degrees();
        // (Beyond square across the section it leaves the other way,
        // turned back.)
        section.flip = angle.abs() > 90.0;
        section.angle = if section.flip {
            angle - 180.0_f64.copysign(angle)
        } else {
            angle
        };
        new
    })?
    .from(at);
    Some(handle.into())
}

/// How a loft leaves section `k`: as full as, and (from a face) turned
/// by, numbers, and the button that flips it.
fn leaving_row(args: &SweptLoftArgs, k: usize) -> Widget<SweptLoftArgs> {
    use workpiece_core::widgets::{HBox, NumericInput, ToolButton};
    let section = &args.sections[k];
    let set = |put: fn(&mut Section, f64)| {
        let now = args.clone();
        move |x: f64| {
            let mut new = now.clone();
            put(&mut new.sections[k], x);
            new
        }
    };
    let mut row: Vec<Widget<SweptLoftArgs>> = vec![
        NumericInput::new(
            "Tangent length ×",
            section.magnitude,
            set(|s, x| s.magnitude = x),
        )
        .into(),
    ];
    if matches!(section.end, SectionEnd::Tangent | SectionEnd::Curvature) {
        row.push(NumericInput::new("Angle °", section.angle, set(|s, x| s.angle = x)).into());
    }
    let flipped = args.clone();
    row.push(
        ToolButton::new(
            "flip",
            "Flip",
            workpiece_core::Hotkey::key(if k == 0 { 'X' } else { 'Z' }),
            move || {
                let mut new = flipped.clone();
                new.sections[k].flip = !new.sections[k].flip;
                new
            },
        )
        .with_hint("Leave the other way: out of the loft")
        .into(),
    );
    HBox::new(row).into()
}

/// The box of the curves it goes along, and what each does.
fn rails_field(args: &SweptLoftArgs, _: &EditContext<'_>) -> Vec<Widget<SweptLoftArgs>> {
    let picked: Vec<Source> = args.rails.iter().map(|r| r.of.source.clone()).collect();
    let was = args.clone();
    let set = move |picked: Vec<Source>| {
        let mut new = was.clone();
        // (A curve picked to one section is its path; to more, a guide.)
        let role = if was.sections.len() >= 2 || was.path().is_some() {
            Role::Guide
        } else {
            Role::Path
        };
        new.rails = picked
            .into_iter()
            .map(|source| {
                was.rails
                    .iter()
                    .find(|r| r.of.source == source)
                    .cloned()
                    .unwrap_or(Rail {
                        of: Reference::new(source),
                        role,
                    })
            })
            .collect();
        new
    };
    let mut out: Vec<Widget<SweptLoftArgs>> = vec![
        ReferenceList::new(
            "Paths and guides",
            vec![ReferenceKind::Profile, ReferenceKind::Body],
            picked,
            set,
        )
        .with_icon("rail-path")
        .into(),
    ];
    use workpiece_core::arguments::Choices;
    for (k, rail) in args.rails.iter().enumerate() {
        let (now, all) = (args.clone(), Role::ALL);
        let choice = Choice::new(
            short(rail.of.name()),
            all.iter().map(|c| c.label().to_owned()).collect(),
            all.iter().position(|c| *c == rail.role).unwrap_or(0),
            move |i| {
                let mut new = now.clone();
                new.rails[k].role = all[i.min(all.len() - 1)];
                new
            },
        )
        .with_icons(all.iter().map(|c| c.icon()).collect())
        .with_icon(rail.role.icon())
        .drop_down();
        out.push(choice.into());
    }
    let said = match (args.sections.len(), args.path().is_some(), args.rails.len()) {
        (_, true, _) => "A path carries the one section along it; twist and scale below",
        (0 | 1, false, 0) => {
            "One section: pick the path it is swept along — or a coil axis below. Two sections or more: a loft through them, which guides shape"
        }
        (_, false, 0) => {
            "A loft through the sections; guide curves it runs along may be picked here"
        }
        _ => {
            "Guides: the loft through the sections runs along them, from the first section to the last"
        }
    };
    out.push(Label::new("", said, Mood::Plain).into());
    out
}
