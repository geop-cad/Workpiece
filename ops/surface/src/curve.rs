//! Curves in space, each a body of its own (a wire) to sweep along or
//! to build on — each way of making one its own step:
//!
//! * [`CurveThrough`]: a smooth curve through picked points, in the
//!   order picked (vertices, frames' origins, points on faces).
//! * [`Helix`]: a helix about an axis, or a spiral.
//! * [`CurveIntersection`]: where two faces cross.
//! * [`CurveProjection`]: curves — edges, curves in space, the curves
//!   of sketches — thrown onto faces along a direction.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, ensure};
use workpiece_boolean::{Solid, read, read_sheet};
use workpiece_core::handles::{AngularHandle, LinearHandle};
use workpiece_core::math::Point3;
use workpiece_core::widgets::{Label, Mood, ReferenceList};
use workpiece_core::world::DatumAxis;
use workpiece_core::world::topo::{Body, BodyId, BodyKind, Curve3, Edge, FaceId, Topo, Vertex};
use workpiece_core::{
    Anything, Builder, Edit, EditContext, EditVisualization, Measure, Operation, Position,
    Reference, ReferenceKind, Resolve, Source, Widget,
};

use workpiece_shape::ui;

fn nothing() -> Measure {
    Measure::new(0.0)
}

/// How a curve through points is held at one of its ends: the way it
/// leaves (or arrives) and how it bends there, where they are given.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EndHeld {
    /// The direction it runs along there (either way along it; it is
    /// taken the way the points go).
    pub along: Option<Point3>,
    /// How it bends there: the curvature vector (towards the centre of
    /// the circle it bends along, as long as one over its radius).
    pub bend: Option<Point3>,
}

/// A smooth curve through `points`, in order. Open, it is one cubic
/// spline through them (curving evenly from one to the next, its
/// parameter how far along the points it is), held at its ends as
/// `ends` say: leaving its first point and arriving at its last along
/// a direction, and bending there as given (curvature continuous with
/// what it runs on from). Closed, it runs on from the last point back
/// to the first, smooth there too: a cubic from each point to the next,
/// leaving every point the way the points beside it lie.
pub fn through(points: &[Point3], closed: bool, ends: [EndHeld; 2]) -> anyhow::Result<Curve3> {
    let n = points.len();
    ensure!(
        n >= 2,
        "a curve goes through two points or more: pick another"
    );
    ensure!(
        !closed || n >= 3,
        "a closed curve goes through three points or more"
    );
    for pair in points.windows(2) {
        ensure!(
            pair[0].distance(pair[1]) > 0.0,
            "two points after each other are one place"
        );
    }
    if !closed {
        return through_open(points, ends);
    }
    ensure!(
        points[0].distance(points[n - 1]) > 0.0,
        "the last point is the first: it closes by itself"
    );
    let mut at: Vec<Point3> = points.to_vec();
    at.push(points[0]);
    // The way through each point: along the chord of its neighbours.
    let way = |k: usize| -> Point3 {
        let k = k % n;
        (points[(k + 1) % n] - points[(k + n - 1) % n]) * 0.5
    };
    let mut control = vec![at[0]];
    let mut knots = vec![0.0; 4];
    for k in 0..n {
        control.push(at[k] + way(k) / 3.0);
        control.push(at[k + 1] - way(k + 1) / 3.0);
        control.push(at[k + 1]);
        let t = (k + 1) as f64;
        knots.extend(if k + 1 == n { vec![t; 4] } else { vec![t; 3] });
    }
    Ok(Curve3::new(3, knots, control, None)?)
}

/// [`through`] an open run of points: the cubic spline through them,
/// by how far along they are, held at its ends.
fn through_open(points: &[Point3], ends: [EndHeld; 2]) -> anyhow::Result<Curve3> {
    use workpiece_shape::interpolate::{Held, interpolate};
    let n = points.len();
    let mut params = vec![0.0];
    for pair in points.windows(2) {
        params.push(params[params.len() - 1] + pair[0].distance(pair[1]));
    }
    // (Its parameter about how far along it is: a direction given is
    // as long as one, a bend given is the curvature vector itself.)
    let row = |p: Point3, w: f64| vec![[p.x, p.y, p.z, w]];
    let held = |end: &EndHeld, forward: Point3| -> Held {
        let along = end
            .along
            .and_then(Point3::try_normalize)
            .map(|d| if d.dot(forward) < 0.0 { -d } else { d });
        let bend = end.bend.zip(along).map(|(k, d)| k - d * k.dot(d));
        Held {
            first: along.map(|d| row(d, 0.0)),
            second: bend.map(|k| row(k, 0.0)),
        }
    };
    let start = held(&ends[0], points[1] - points[0]);
    let end = held(&ends[1], points[n - 1] - points[n - 2]);
    let rows = points.iter().map(|p| row(*p, 1.0)).collect();
    let (degree, knots, control) = interpolate(&params, rows, [&start, &end])?;
    let control = control
        .iter()
        .map(|row| Point3::new(row[0][0], row[0][1], row[0][2]))
        .collect();
    Ok(Curve3::new(degree, knots, control, None)?)
}

/// The kinds of reference a curve through points goes through.
const THROUGH: [ReferenceKind; 3] = [
    ReferenceKind::Vertex,
    ReferenceKind::Point,
    ReferenceKind::Face,
];

/// How a curve through points runs at one of its ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum EndWay {
    /// As the points lie.
    #[default]
    Free,
    /// Along an edge or an axis (G1): tangent to it where it is nearest.
    Tangent,
    /// Along an edge, and bending as it does there (G2): it runs on
    /// from the edge smoothly, its curvature too.
    Curvature,
    /// Lying in a face or a plane: tangent to it, the way the points
    /// lean along it.
    InFace,
    /// Square to a face or a plane.
    Normal,
}

impl workpiece_core::arguments::Choices for EndWay {
    const ALL: &'static [Self] = &[
        Self::Free,
        Self::Tangent,
        Self::Curvature,
        Self::InFace,
        Self::Normal,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Tangent => "Tangent (G1)",
            Self::Curvature => "Curvature (G2)",
            Self::InFace => "In face",
            Self::Normal => "Normal",
        }
    }

    fn icon(self) -> Option<&'static str> {
        Some(match self {
            Self::Free => "end-free",
            Self::Tangent => "end-tangent",
            Self::Curvature => "end-curvature",
            Self::InFace => "end-in-face",
            Self::Normal => "end-normal",
        })
    }
}

impl EndWay {
    /// What it is held to: lines and edges, or faces and planes.
    fn kinds(self) -> Vec<ReferenceKind> {
        match self {
            Self::Free => Vec::new(),
            Self::Tangent => vec![ReferenceKind::Edge, ReferenceKind::Axis],
            Self::Curvature => vec![ReferenceKind::Edge],
            Self::InFace | Self::Normal => vec![ReferenceKind::Face, ReferenceKind::Plane],
        }
    }
}

/// How a curve through points runs at one of its ends, and what it is
/// held to there.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    workpiece_core::Arguments,
)]
pub struct CurveEnd {
    /// How it runs.
    #[serde(default)]
    #[arg(hidden)]
    pub way: EndWay,
    /// What it is held to: an edge or an axis (tangent, curvature), a
    /// face or a plane (in face, normal).
    #[serde(default)]
    #[arg(hidden)]
    pub to: Option<Reference<Anything>>,
}

/// The direction the curve `curve` runs along at its parameter `t`, and
/// its curvature vector there.
fn bend_of(curve: &Curve3, t: f64) -> (Point3, Point3) {
    let (a, b) = curve.domain();
    let h = (b - a) * 1e-5;
    let (lo, hi) = ((t - h).max(a), (t + h).min(b));
    let d = curve.derivative_at(t).1;
    let dd = (curve.derivative_at(hi).1 - curve.derivative_at(lo).1) / (hi - lo);
    let speed = d.length_squared();
    if speed == 0.0 {
        return (d, Point3::ZERO);
    }
    let unit = d / speed.sqrt();
    (unit, (dd - unit * dd.dot(unit)) / speed)
}

/// The parameter of `curve` within `range` nearest `to`.
fn nearest_on(curve: &Curve3, (a, b): (f64, f64), to: Point3) -> f64 {
    let mut best = (f64::INFINITY, a);
    for k in 0..=256 {
        let t = a + (b - a) * f64::from(k) / 256.0;
        let far = curve.point_at(t).distance(to);
        if far < best.0 {
            best = (far, t);
        }
    }
    let mut t = best.1;
    for _ in 0..30 {
        let (p, d) = curve.derivative_at(t);
        let step = (to - p).dot(d) / d.length_squared().max(1e-300);
        t = (t + step).clamp(a.min(b), a.max(b));
        if step.abs() <= (b - a).abs() * 1e-14 {
            break;
        }
    }
    t
}

impl CurveEnd {
    /// How the curve is held at its end `at`, the points lying `free`
    /// from it, in `world`.
    fn held(
        &self,
        world: &workpiece_core::World,
        at: Point3,
        free: Point3,
    ) -> anyhow::Result<EndHeld> {
        if self.way == EndWay::Free {
            return Ok(EndHeld::default());
        }
        let to = self.to.as_ref().context("pick what the end is held to")?;
        let source = &to.source;
        let edge = match source {
            Source::Topology(name) => world
                .topo
                .resolve::<workpiece_core::world::topo::EdgeId>(name)
                .and_then(|e| world.topo.edge(e))
                .and_then(|e| Some((world.topo.curve(e.curve)?.clone(), (e.t0, e.t1)))),
            _ => None,
        };
        let face = match source {
            Source::Topology(name) if world.topo.resolve::<FaceId>(name).is_some() => {
                Some(face_sheet(&world.topo, name)?)
            }
            _ => None,
        };
        Ok(match self.way {
            EndWay::Free => EndHeld::default(),
            EndWay::Tangent | EndWay::Curvature => {
                let (along, bend) = match edge {
                    Some((curve, range)) => bend_of(&curve, nearest_on(&curve, range, at)),
                    None => {
                        ensure!(
                            self.way == EndWay::Tangent,
                            "a curve bends as an edge does: pick an edge"
                        );
                        let axis: Reference<DatumAxis> = Reference::new(source.clone());
                        (world.resolve(&axis)?.direction(), Point3::ZERO)
                    }
                };
                EndHeld {
                    along: Some(along),
                    bend: (self.way == EndWay::Curvature).then_some(bend),
                }
            }
            EndWay::InFace | EndWay::Normal => {
                let normal = match &face {
                    Some(sheet) => nearest(&sheet.faces[0], at).1,
                    None => {
                        let plane: Reference<workpiece_core::world::DatumPlane> =
                            Reference::new(source.clone());
                        world.resolve(&plane)?.normal()
                    }
                };
                let along = if self.way == EndWay::Normal {
                    normal
                } else {
                    let lying = free - normal * free.dot(normal);
                    lying
                        .try_normalize()
                        .context("the points run square to the face: the curve cannot lie in it")?
                };
                EndHeld {
                    along: Some(along),
                    bend: None,
                }
            }
        })
    }
}

/// What a curve through points goes through, and how.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct CurveThroughArgs {
    /// What it goes through, in order: vertices, frames (their
    /// origins), and faces (at a point on each).
    #[arg(with = "through_field")]
    pub points: Vec<Reference<Anything>>,
    /// Where on the face at the same place in `points` it goes through
    /// (as the face was when it was pointed at; the point of the face
    /// nearest that, as the face is now); nothing for what is a point
    /// by itself.
    #[serde(default)]
    #[arg(hidden)]
    pub spots: Vec<Option<[f64; 3]>>,
    /// How far off the faces it goes through its points on them — a
    /// cable's radius, laid along them.
    #[serde(default = "nothing")]
    #[arg(
        label = "Off the faces",
        when = "self.spots.iter().any(Option::is_some)"
    )]
    pub offset: Measure,
    /// Whether it runs on from the last point back to the first.
    #[serde(default)]
    #[arg(
        label = "Ends",
        options = "Open|Closed",
        icons = "ends-open|ends-closed"
    )]
    pub closed: bool,
    /// How it leaves its first point (of an open one).
    #[serde(default)]
    #[arg(nested, with = "start_fields", when = "!self.closed")]
    pub start: CurveEnd,
    /// How it arrives at its last point.
    #[serde(default)]
    #[arg(nested, with = "end_fields", when = "!self.closed")]
    pub end: CurveEnd,
}

impl Default for CurveThroughArgs {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            spots: Vec::new(),
            offset: nothing(),
            closed: false,
            start: CurveEnd::default(),
            end: CurveEnd::default(),
        }
    }
}

/// The fields of how a curve through points runs at an end: the way,
/// as pictures, and what it is held to.
fn end_widgets(
    args: &CurveThroughArgs,
    label: &'static str,
    of: fn(&mut CurveThroughArgs) -> &mut CurveEnd,
) -> Vec<Widget<CurveThroughArgs>> {
    let now = of(&mut args.clone()).clone();
    let context_free = args.clone();
    // (Its caption says which way it is: the pictures alone do not.)
    use workpiece_core::arguments::Choices;
    let all = EndWay::ALL;
    let way = workpiece_core::widgets::Choice::new(
        format!("{label}: {}", now.way.label()),
        all.iter().map(|w| w.label().to_owned()).collect(),
        all.iter().position(|w| *w == now.way).unwrap_or(0),
        move |i: usize| {
            let way = all[i.min(all.len() - 1)];
            let mut new = context_free.clone();
            let end = of(&mut new);
            if end.way.kinds() != way.kinds() {
                end.to = None;
            }
            end.way = way;
            new
        },
    )
    .with_icons(all.iter().map(|w| w.icon()).collect())
    .with_icon("f-extent");
    let mut out = vec![way.into()];
    if now.way != EndWay::Free {
        let kinds = now.way.kinds();
        let said = match now.way {
            EndWay::Tangent => "Edge or axis",
            EndWay::Curvature => "Edge",
            _ => "Face or plane",
        };
        let (set, clear) = (args.clone(), args.clone());
        let input = workpiece_core::widgets::ReferenceInput::of_kind(
            said,
            kinds[0],
            now.to.as_ref().map(|r| r.source.clone()),
            move |source: Source| {
                let mut new = set.clone();
                of(&mut new).to = Some(source.into());
                new
            },
        )
        .taking(&kinds)
        .optional("Pick what it is held to", move || {
            let mut new = clear.clone();
            of(&mut new).to = None;
            new
        });
        out.push(input.into());
    }
    out
}

fn start_fields(args: &CurveThroughArgs, _: &EditContext<'_>) -> Vec<Widget<CurveThroughArgs>> {
    end_widgets(args, "Start", |a| &mut a.start)
}

fn end_fields(args: &CurveThroughArgs, _: &EditContext<'_>) -> Vec<Widget<CurveThroughArgs>> {
    end_widgets(args, "End", |a| &mut a.end)
}

/// Adds a smooth curve through picked points, in the order picked, as
/// a wire body named after the operation's id (its edge `{id}/e0`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CurveThrough {
    /// The operation's id; also the body's name.
    pub id: String,
    /// What it goes through.
    pub args: CurveThroughArgs,
}

impl CurveThroughArgs {
    /// The points the curve goes through, in `world`.
    fn places(&self, builder: &dyn Builder) -> anyhow::Result<Vec<Point3>> {
        let world = builder.tracked();
        let offset = (*builder).measure(&self.offset, "the distance off the faces")?;
        let mut points = Vec::new();
        for (k, source) in self.points.iter().enumerate() {
            points.push(match self.spots.get(k).copied().flatten() {
                // A point on a face: where the face is nearest to where
                // it was put, and off it.
                Some(at) => {
                    let sheet = face_sheet(&world.topo, source.name())?;
                    let (on, out) = nearest(&sheet.faces[0], Point3::from_array(at));
                    on + out * offset
                }
                None => Position::resolve(world, source)?.0,
            });
        }
        Ok(points)
    }

    /// The curve, through `points`.
    fn curve(&self, world: &workpiece_core::World, points: &[Point3]) -> anyhow::Result<Curve3> {
        let n = points.len();
        let ends = if self.closed || n < 2 {
            [EndHeld::default(); 2]
        } else {
            [
                self.start.held(world, points[0], points[1] - points[0])?,
                self.end
                    .held(world, points[n - 1], points[n - 1] - points[n - 2])?,
            ]
        };
        through(points, self.closed, ends)
    }
}

impl Operation for CurveThrough {
    type Args = CurveThroughArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &CurveThroughArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut CurveThroughArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let points = self.args.places(builder)?;
        let curve = self.args.curve(builder.tracked(), &points)?;
        Ok(wire(&self.id, whole(curve)))
    }

    fn summary(&self) -> String {
        let closed = if self.args.closed { ", closed" } else { "" };
        format!("through {} points{closed}", self.args.points.len())
    }
}

/// The box of what the curve goes through: what is picked is added in
/// order (a face at the point pointed at; again, at another); picking a
/// point that is in it already takes it out.
fn through_field(
    args: &CurveThroughArgs,
    context: &EditContext<'_>,
) -> Vec<Widget<CurveThroughArgs>> {
    let picked = Reference::sources(&args.points);
    let set = ui::set(args, |a, picked: Vec<Source>| {
        // What went out of the box takes its place on a face with it.
        let was = Reference::sources(&a.points);
        let mut spots = a.spots.clone();
        spots.resize(was.len(), None);
        if picked.len() + 1 == was.len() {
            let gone = (0..picked.len())
                .find(|&k| picked[k] != was[k])
                .unwrap_or(picked.len());
            spots.remove(gone);
        } else {
            spots.resize(picked.len(), None);
        }
        a.spots = spots;
        a.points = Reference::each(picked);
    });
    let at = {
        let (args, world) = (args.clone(), context.world.clone());
        move |source: Source, at: Point3| {
            let mut a = args.clone();
            a.spots.resize(a.points.len(), None);
            let face = matches!(&source, Source::Topology(name)
                if world.topo.resolve::<FaceId>(name).is_some());
            let held = a.points.iter().position(|p| p.source == source);
            match held {
                // A point picked again goes out; a face takes another
                // point on it.
                Some(k) if !face => {
                    a.points.remove(k);
                    a.spots.remove(k);
                }
                _ => {
                    a.points.push(source.into());
                    a.spots.push(face.then(|| at.to_array()));
                }
            }
            a
        }
    };
    let count = args.points.len();
    let said = match count {
        0 => "Pick the points in order: vertices, frames, or places on faces".to_owned(),
        1 => "1 point: pick the next".to_owned(),
        n => format!("{n} points, in the order picked"),
    };
    vec![
        ReferenceList::new("Through", THROUGH.to_vec(), picked, set)
            .required()
            .picked_at(at)
            .into(),
        Label::new("", said, Mood::Plain).into(),
    ]
}

/// A helix's axis, radius, rise, turns and growth.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct HelixArgs {
    /// The axis it turns about.
    pub axis: Option<Reference<DatumAxis>>,
    /// Its radius where it starts.
    pub radius: Measure,
    /// How far it advances along the axis in a turn; nothing for a flat
    /// spiral.
    pub pitch: Measure,
    /// How many turns it makes.
    pub turns: Measure,
    /// How much its radius grows in a turn: a spiral.
    #[serde(default = "nothing")]
    #[arg(label = "Grows a turn")]
    pub grow: Measure,
    /// Where about the axis it starts, in degrees from the axis's own
    /// zero direction (a frame's axis: its next axis; z: x).
    #[serde(default = "nothing")]
    #[arg(label = "Start angle °")]
    pub start: Measure,
    /// Whether it is a left-handed helix.
    #[serde(default)]
    #[arg(flip, row)]
    pub left: bool,
}

impl Default for HelixArgs {
    fn default() -> Self {
        Self {
            axis: None,
            radius: Measure::new(10.0),
            pitch: Measure::new(5.0),
            turns: Measure::new(3.0),
            grow: nothing(),
            start: nothing(),
            left: false,
        }
    }
}

/// Adds a helix about an axis (or, growing as it turns, a spiral) as a
/// wire body named after the operation's id (its edge `{id}/e0`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Helix {
    /// The operation's id; also the body's name.
    pub id: String,
    /// Its axis and sizes.
    pub args: HelixArgs,
}

/// Where a helix is: its axis's point and direction, the direction it
/// starts in from the axis and its zero direction, and the way it turns
/// about the axis.
struct Laid {
    origin: Point3,
    axis: Point3,
    zero: Point3,
    start: Point3,
    sense: f64,
}

impl Laid {
    fn of(axis: &DatumAxis, start_degrees: f64, left: bool) -> Self {
        let (origin, axis) = (axis.origin(), axis.direction().normalize_or_zero());
        let zero = axis.any_orthonormal_vector();
        let start = glam::DQuat::from_axis_angle(axis, start_degrees.to_radians()) * zero;
        Self {
            origin,
            axis,
            zero,
            start,
            sense: if left { -1.0 } else { 1.0 },
        }
    }

    /// The direction from the axis after `turns` turns.
    fn after(&self, turns: f64) -> Point3 {
        let angle = self.sense * turns * std::f64::consts::TAU;
        glam::DQuat::from_axis_angle(self.axis, angle) * self.start
    }
}

impl Operation for Helix {
    type Args = HelixArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &HelixArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut HelixArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let args = &self.args;
        let radius = (*builder).measure(&args.radius, "the radius")?;
        let pitch = (*builder).measure(&args.pitch, "the pitch")?;
        let turns = (*builder).measure(&args.turns, "the turns")?;
        let grow = (*builder).measure(&args.grow, "the growth")?;
        let start = (*builder).measure(&args.start, "the start angle")?;
        ensure!(radius > 0.0, "a helix has some radius");
        ensure!(
            pitch != 0.0 || grow != 0.0,
            "a helix advances as it turns, or a spiral grows"
        );
        let axis = args.axis.as_ref().context("pick the axis of the helix")?;
        let axis = builder.tracked().resolve(axis)?;
        let laid = Laid::of(&axis, start, args.left);
        let from = laid.origin + laid.start * radius;
        let curve = workpiece_shape::swept::helix(
            laid.origin,
            laid.axis,
            from,
            pitch,
            turns,
            grow,
            args.left,
        )?;
        Ok(wire(&self.id, whole(curve)))
    }

    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        (): &(),
    ) -> EditVisualization<Edit<HelixArgs, ()>> {
        let args = &self.args;
        let fields = workpiece_core::Arguments::fields(args, context);
        let mut handles = BTreeMap::new();
        let value = |m: &Measure| m.resolve(context.parameters).unwrap_or(m.value);
        let axis = args
            .axis
            .as_ref()
            .and_then(|a| context.world.resolve(a).ok());
        if let Some(axis) = axis {
            let (radius, pitch, turns, grow, start) = (
                value(&args.radius),
                value(&args.pitch),
                value(&args.turns),
                value(&args.grow),
                value(&args.start),
            );
            let laid = Laid::of(&axis, start, args.left);
            let height = pitch * turns;
            let top = laid.origin + laid.axis * height;
            let end = laid.after(turns);
            let mut add =
                |name: &str, handle: Option<workpiece_core::handles::Handle<HelixArgs>>| {
                    if let Some(handle) = handle {
                        handles.insert(name.to_owned(), handle);
                    }
                };
            // Its radius where it starts, out from the axis to the start.
            let set = ui::set(args, |a, r: f64| a.radius = Measure::new(r.max(1e-9)));
            add(
                "radius",
                LinearHandle::new(laid.origin, laid.start, radius, set).map(Into::into),
            );
            // (The rings that turn it stand a little off its ends along
            // the axis: their knobs clear of the arrows at its ends.)
            let apart = laid.axis * (0.3 * radius.abs().max(height.abs() * 0.1));
            // Where it starts, turned about the axis.
            let set = ui::set(args, |a, degrees: f64| a.start = Measure::new(degrees));
            add(
                "start",
                AngularHandle::new(laid.origin - apart, laid.axis, laid.zero, start, set)
                    .map(Into::into),
            );
            // How high it rises, up the axis: the pitch, for the turns it
            // makes.
            let set = ui::set(args, move |a, h: f64| {
                if turns > 0.0 {
                    a.pitch = Measure::new(h / turns);
                }
            });
            add(
                "height",
                LinearHandle::new(laid.origin, laid.axis, height, set).map(Into::into),
            );
            // How many turns, turned about the axis at its top.
            let set = ui::set(args, |a, degrees: f64| {
                a.turns = Measure::new((degrees / 360.0).max(0.01));
            });
            add(
                "turns",
                AngularHandle::new(
                    top + apart,
                    laid.axis * laid.sense,
                    laid.start,
                    turns * 360.0,
                    set,
                )
                .map(Into::into),
            );
            // Its radius where it ends: out from the axis at its top;
            // more than where it starts, a spiral that grows.
            let set = ui::set(args, move |a, r: f64| {
                if turns > 0.0 {
                    a.grow = Measure::new((r - radius) / turns);
                }
            });
            add(
                "end radius",
                LinearHandle::new(top, end, radius + grow * turns, set).map(Into::into),
            );
        }
        EditVisualization::new(workpiece_core::widgets::VBox::new(fields), handles).map(Edit::args)
    }

    fn summary(&self) -> String {
        let a = &self.args;
        if a.grow.value != 0.0 && a.pitch.value == 0.0 {
            format!("a spiral of {} turns", a.turns)
        } else {
            format!("a helix of {} turns", a.turns)
        }
    }
}

/// The two faces a crossing is of.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    workpiece_core::Arguments,
)]
pub struct CurveIntersectionArgs {
    /// One face.
    #[arg(label = "First face")]
    pub first: Option<Reference<FaceId>>,
    /// The other.
    #[arg(label = "Second face")]
    pub second: Option<Reference<FaceId>>,
}

/// Adds where two faces cross as a wire body named after the
/// operation's id: its edges `{id}/e0`, `{id}/e1`, … one a piece.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CurveIntersection {
    /// The operation's id; also the body's name.
    pub id: String,
    /// The faces.
    pub args: CurveIntersectionArgs,
}

impl Operation for CurveIntersection {
    type Args = CurveIntersectionArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &CurveIntersectionArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut CurveIntersectionArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let args = &self.args;
        let first = args.first.as_ref().context("pick the first face")?;
        let second = args.second.as_ref().context("pick the second face")?;
        let topo = &builder.tracked().topo;
        let pieces = crossing(
            &face_sheet(topo, first.name())?,
            &face_sheet(topo, second.name())?,
        )?;
        Ok(wire(&self.id, pieces))
    }

    fn summary(&self) -> String {
        "where two faces cross".to_owned()
    }
}

/// What is thrown onto which faces, and along what.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    workpiece_core::Arguments,
)]
pub struct CurveProjectionArgs {
    /// The faces it is thrown onto.
    #[arg(label = "Onto faces", required)]
    pub onto: Vec<Reference<FaceId>>,
    /// What is thrown: edges, curves in space (wire bodies), sketches
    /// (all their curves).
    #[arg(with = "curves_field")]
    pub curves: Vec<Reference<Anything>>,
    /// The direction it is thrown along; none for a sketch's normal.
    #[arg(label = "Along", empty = "Normal of the sketch")]
    pub along: Option<Reference<DatumAxis>>,
}

/// Adds curves thrown onto faces along a direction — edges, curves in
/// space, the curves of sketches — as a wire body named after the
/// operation's id: its edges `{id}/e0`, `{id}/e1`, … one a piece.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CurveProjection {
    /// The operation's id; also the body's name.
    pub id: String,
    /// What, onto what, along what.
    pub args: CurveProjectionArgs,
}

/// The kinds of what a projection throws.
const THROWN: [ReferenceKind; 3] = [
    ReferenceKind::Profile,
    ReferenceKind::Edge,
    ReferenceKind::Body,
];

/// The box of what is thrown onto the faces.
fn curves_field(
    args: &CurveProjectionArgs,
    _: &EditContext<'_>,
) -> Vec<Widget<CurveProjectionArgs>> {
    let set = ui::set(args, |a, picked: Vec<Source>| {
        a.curves = Reference::each(picked)
    });
    vec![
        ReferenceList::new(
            "Project",
            THROWN.to_vec(),
            Reference::sources(&args.curves),
            set,
        )
        .required()
        .into(),
    ]
}

impl Operation for CurveProjection {
    type Args = CurveProjectionArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &CurveProjectionArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut CurveProjectionArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let args = &self.args;
        let world = builder.tracked();
        let topo = &world.topo;
        ensure!(!args.onto.is_empty(), "pick the faces to throw onto");
        ensure!(!args.curves.is_empty(), "pick what to throw onto the faces");
        let along = args
            .along
            .as_ref()
            .map(|a| Ok::<_, anyhow::Error>(world.resolve(a)?.direction()))
            .transpose()?;
        // What is thrown, each curve with its range and the direction it
        // goes along.
        let mut thrown: Vec<(Curve3, (f64, f64), Point3)> = Vec::new();
        for source in &args.curves {
            match &source.source {
                Source::Profile(name) => {
                    let profile = world
                        .profiles
                        .get(name)
                        .with_context(|| format!("there is no sketch {name:?}"))?;
                    let frame = workpiece_shape::sweep::Frame::of(profile);
                    let way = along.unwrap_or_else(|| frame.normal());
                    for edge in &profile.edges {
                        let curve = &edge.curve;
                        let points = curve
                            .control_points()
                            .iter()
                            .map(|&p| frame.place(p))
                            .collect();
                        let lifted = Curve3::new(
                            curve.degree(),
                            curve.knots().to_vec(),
                            points,
                            Some(curve.weights().to_vec()),
                        )?;
                        thrown.push((lifted, (edge.t0, edge.t1), way));
                    }
                }
                Source::Topology(name) => {
                    let way = along.context("pick the direction to throw it along")?;
                    let edges: Vec<workpiece_core::world::topo::EdgeId> =
                        match topo.resolve::<BodyId>(name).and_then(|b| topo.body(b)) {
                            Some(body) if body.kind == BodyKind::Wire => body.free_edges.clone(),
                            Some(_) => {
                                anyhow::bail!("{name:?} is a body, but no curve: pick its edges")
                            }
                            None => {
                                vec![topo.resolve(name).with_context(|| {
                                    format!("there is no edge or curve {name:?}")
                                })?]
                            }
                        };
                    for id in edges {
                        let edge = topo.edge(id).context("an edge is gone")?;
                        let line = topo.curve(edge.curve).context("an edge without a curve")?;
                        thrown.push((line.clone(), (edge.t0, edge.t1), way));
                    }
                }
                other => anyhow::bail!("{} cannot be thrown onto a face", other.described()),
            }
        }
        let faces: Vec<Solid> = args
            .onto
            .iter()
            .map(|f| face_sheet(topo, f.name()))
            .collect::<anyhow::Result<_>>()?;
        let mut found = Vec::new();
        for (k, (line, range, way)) in thrown.iter().enumerate() {
            let way = way.try_normalize().context("the direction has no length")?;
            for onto in &faces {
                // The curve carried along the direction, both ways, as
                // far as the face is: where that wall crosses the face.
                let (lo, hi) = onto.extent();
                let from = line.point_at(range.0);
                let size = (hi - lo).length();
                let corners = (0..8).map(|c| {
                    let pick = |bit: usize, a: f64, b: f64| if c & bit == 0 { a } else { b };
                    Point3::new(
                        pick(1, lo.x, hi.x),
                        pick(2, lo.y, hi.y),
                        pick(4, lo.z, hi.z),
                    )
                });
                let (near, far) = corners.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
                    let s = (p - from).dot(way);
                    (a.min(s), b.max(s))
                });
                let (near, far) = (near - size * 0.1, far + size * 0.1);
                let wall = workpiece_shape::ruled(
                    line,
                    *range,
                    way * (far - near),
                    &format!("{}/w{k}", self.id),
                )?
                .moved(&|p| p + way * near)?;
                if let Ok(pieces) = crossing(&wall, onto) {
                    found.extend(pieces);
                }
            }
        }
        ensure!(
            !found.is_empty(),
            "along the direction, the curves miss the faces"
        );
        Ok(wire(&self.id, found))
    }

    fn summary(&self) -> String {
        let n = self.args.curves.len();
        format!(
            "{n} {} thrown onto {} faces",
            if n == 1 { "curve" } else { "curves" },
            self.args.onto.len()
        )
    }
}

/// All of `curve`.
fn whole(curve: Curve3) -> Vec<Piece> {
    let range = curve.domain();
    vec![(Arc::new(curve), range)]
}

/// What a curve step makes: the wire body `id` of `curves`.
fn wire(id: &str, curves: Vec<Piece>) -> workpiece_core::Made<'static> {
    let made = move |topo: &mut Topo, name: &str| write_wire(topo, name, &curves).map(|_| ());
    workpiece_core::Made::body(id, made)
}

/// A curve, and the part of it that counts.
pub type Piece = (Arc<Curve3>, (f64, f64));

/// Where the sheets `a` and `b` cross: the pieces of the crossing that
/// are on both, each a curve and the part of it that counts.
pub fn crossing(a: &Solid, b: &Solid) -> anyhow::Result<Vec<Piece>> {
    let reach = {
        let ((lo, hi), (lo2, hi2)) = (a.extent(), b.extent());
        (hi.max(hi2) - lo.min(lo2)).length()
    };
    ensure!(reach > 0.0, "the faces have some size");
    // Where the faces meet: where they cross, and where an edge of one
    // lies in the other (two faces of a body side by side meet along
    // the edge between them).
    let found = workpiece_boolean::meeting_curves(a, b)
        .map_err(|why| anyhow::anyhow!("where the faces cross could not be told: {why}"))?;
    let found: Vec<Piece> = found
        .into_iter()
        .filter(|(c, (t0, t1))| {
            let (p, q) = (c.point_at(*t0), c.point_at(*t1));
            let middle = c.point_at((t0 + t1) / 2.0);
            p.distance(q).max(p.distance(middle)) > reach * 1e-9
        })
        .collect();
    ensure!(!found.is_empty(), "the faces do not cross in a curve");
    Ok(found)
}

/// Adds `curves` (each with the part of it that counts) to `topo` as a
/// wire body called `name`, its edges `{name}/e0`, …; ends at one place
/// are one vertex.
pub fn write_wire(topo: &mut Topo, name: &str, curves: &[Piece]) -> anyhow::Result<BodyId> {
    ensure!(!curves.is_empty(), "there is no curve");
    let ends: Vec<[Point3; 2]> = curves
        .iter()
        .map(|(c, (t0, t1))| [c.point_at(*t0), c.point_at(*t1)])
        .collect();
    let size = ends
        .iter()
        .flatten()
        .fold(0.0_f64, |m, p| m.max(p.length()));
    let longest = ends.iter().map(|e| e[0].distance(e[1])).fold(0.0, f64::max);
    let tolerance = size.max(longest).max(1e-9) * 1e-9;
    let mut vertices: Vec<(Point3, workpiece_core::world::topo::VertexId)> = Vec::new();
    let mut edges = Vec::new();
    for (k, (curve, (t0, t1))) in curves.iter().enumerate() {
        let mut at = |topo: &mut Topo, p: Point3| -> anyhow::Result<_> {
            if let Some((_, v)) = vertices
                .iter()
                .find(|(q, _)| q.distance(p) <= tolerance * 100.0)
            {
                return Ok(*v);
            }
            let v = topo.add_vertex(Vertex {
                point: p,
                tolerance: tolerance * 100.0,
            })?;
            vertices.push((p, v));
            Ok(v)
        };
        let (start, end) = (at(topo, ends[k][0])?, at(topo, ends[k][1])?);
        let curve = topo.add_curve((**curve).clone());
        let edge = topo.add_edge(Edge {
            curve,
            t0: *t0,
            t1: *t1,
            start,
            end,
            tolerance,
        })?;
        topo.set_name(edge, format!("{name}/e{k}"))?;
        edges.push(edge);
    }
    let body = topo.add_body(Body {
        kind: BodyKind::Wire,
        lumps: Vec::new(),
        free_edges: edges,
        free_vertices: Vec::new(),
    })?;
    topo.set_name(body, name)?;
    Ok(body)
}

/// The point of `face`'s surface nearest `to`, and the way out of the
/// body there.
fn nearest(face: &workpiece_boolean::model::Face, to: Point3) -> (Point3, Point3) {
    let surface = &face.surface;
    let ((u0, u1), (v0, v1)) = surface.domain();
    // From the nearest of a grid of its points, closer step by step.
    const GRID: usize = 16;
    let mut best = (f64::INFINITY, u0, v0);
    for i in 0..=GRID {
        for j in 0..=GRID {
            let u = u0 + (u1 - u0) * i as f64 / GRID as f64;
            let v = v0 + (v1 - v0) * j as f64 / GRID as f64;
            let far = surface.point_at(u, v).distance(to);
            if far < best.0 {
                best = (far, u, v);
            }
        }
    }
    let (_, mut u, mut v) = best;
    for _ in 0..40 {
        let (p, du, dv) = surface.derivatives_at(u, v);
        let off = to - p;
        // (The step in u and v that takes the point nearest in the
        // tangent plane.)
        let (a, b, c) = (du.dot(du), du.dot(dv), dv.dot(dv));
        let (r, s) = (off.dot(du), off.dot(dv));
        let det = a * c - b * b;
        if det.abs() < 1e-300 {
            break;
        }
        let (su, sv) = ((r * c - s * b) / det, (s * a - r * b) / det);
        u = (u + su).clamp(u0, u1);
        v = (v + sv).clamp(v0, v1);
        if su.abs() <= 1e-14 * (u1 - u0) && sv.abs() <= 1e-14 * (v1 - v0) {
            break;
        }
    }
    let (p, du, dv) = surface.derivatives_at(u, v);
    let normal = du.cross(dv).normalize_or_zero();
    (p, if face.outward { normal } else { -normal })
}

/// The sheet of the one face called `name`, of whatever body has it.
fn face_sheet(topo: &Topo, name: &str) -> anyhow::Result<Solid> {
    for (id, body) in topo.bodies().iter() {
        let solid = match body.kind {
            BodyKind::Solid => read(topo, id),
            BodyKind::Sheet => read_sheet(topo, id),
            _ => continue,
        };
        let Ok(solid) = solid else {
            continue;
        };
        let found = solid
            .faces
            .iter()
            .position(|f| f.name.as_deref() == Some(name) || f.also.iter().any(|n| n == name));
        if let Some(face) = found {
            return Ok(workpiece_shape::faces_of(&solid, &[face]));
        }
    }
    anyhow::bail!("there is no face {name:?}")
}
