//! Transform: a body moved, turned, mirrored or scaled (or a copy of
//! it); and where the copies of a pattern go.

use anyhow::{Context, ensure};
use workpiece_boolean::{Solid, read};
use workpiece_core::math::angle_between;
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{BodyId, Curve3, FaceId, Topo};
use workpiece_core::world::{DatumAxis, DatumPlane};
use workpiece_core::{Builder, Measure, Operation, Reference};

use workpiece_shape::ui;

use crate::fill::Grid;

/// The body `source` names in `topo`, and its name.
fn body_of(
    topo: &Topo,
    source: &Option<ui::BodyRef>,
    what: &str,
) -> anyhow::Result<(BodyId, String)> {
    let name = source
        .as_ref()
        .with_context(|| format!("pick {what}"))?
        .name();
    let id = topo
        .resolve::<BodyId>(name)
        .with_context(|| format!("there is no body {name:?}"))?;
    Ok((id, name.to_owned()))
}

/// `p` turned by `angle` (radians) about `axis`.
fn turned(axis: &DatumAxis, angle: f64, p: Point3) -> Point3 {
    let (o, d) = (axis.origin(), axis.direction());
    let r = p - o;
    let along = d * r.dot(d);
    let across = r - along;
    o + along + across * libm::cos(angle) + d.cross(across) * libm::sin(angle)
}

/// How a body is transformed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Motion {
    /// Shifted by a distance along each of the world's axes.
    #[default]
    Move,
    /// Turned about an axis.
    Rotate,
    /// Reflected in a plane.
    Mirror,
    /// Grown or shrunk about the origin of the world.
    Scale,
}

const MOTIONS: [Motion; 4] = [Motion::Move, Motion::Rotate, Motion::Mirror, Motion::Scale];

impl Motion {
    fn label(self) -> &'static str {
        match self {
            Motion::Move => "Move",
            Motion::Rotate => "Rotate",
            Motion::Mirror => "Mirror",
            Motion::Scale => "Scale",
        }
    }
}

/// The body, how it is transformed, and whether it stays where it was
/// too.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct TransformArgs {
    /// The body.
    pub body: Option<workpiece_core::Reference<workpiece_core::world::topo::BodyId>>,
    /// How it is transformed.
    #[arg(choice, label = "How")]
    pub motion: Motion,
    /// Moving: how far along x, y and z.
    #[arg(field, when = "self.motion == Motion::Move")]
    pub by: [Measure; 3],
    /// Rotating: the axis.
    #[arg(label = "About", when = "self.motion == Motion::Rotate")]
    pub axis: Option<Reference<DatumAxis>>,
    /// Rotating: the angle, in degrees.
    #[arg(label = "Angle °", when = "self.motion == Motion::Rotate")]
    pub angle: Measure,
    /// Mirroring: the plane.
    #[arg(label = "In", when = "self.motion == Motion::Mirror")]
    pub plane: Option<Reference<DatumPlane>>,
    /// Scaling: by how much.
    #[arg(when = "self.motion == Motion::Scale")]
    pub factor: Measure,
    /// Whether the body stays and a copy of it is transformed.
    #[arg(
        label = "Keep",
        options = "Move the body|Make a copy",
        icons = "keep-move|keep-copy"
    )]
    pub copy: bool,
}

impl Default for TransformArgs {
    fn default() -> Self {
        Self {
            body: None,
            motion: Motion::Move,
            by: [0.0, 0.0, 1.0].map(Measure::new),
            axis: None,
            angle: Measure::new(90.0),
            plane: None,
            factor: Measure::new(2.0),
            copy: false,
        }
    }
}

/// Moves, turns, mirrors or scales a body — or a copy of it, which is a
/// body of its own called after the operation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Transform {
    /// The operation's id.
    pub id: String,
    /// What is transformed, and how.
    pub args: TransformArgs,
}

impl Operation for Transform {
    type Args = TransformArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &TransformArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut TransformArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = workpiece_shape::bodies::Out::default();
        let args = &self.args;
        let (body, name) = body_of(&builder.tracked().topo, &args.body, "the body to transform")?;
        // (A surface is moved as a solid is.)
        let sheet = builder.tracked().topo.body(body).map(|b| b.kind)
            == Some(workpiece_core::world::topo::BodyKind::Sheet);
        let solid = if sheet {
            workpiece_boolean::read_sheet(&builder.tracked().topo, body)?
        } else {
            read(&builder.tracked().topo, body)?
        };
        let made = match args.motion {
            Motion::Move => {
                let mut by = Point3::ZERO;
                for (k, what) in ["x", "y", "z"].into_iter().enumerate() {
                    by[k] = (*builder).measure(&args.by[k], what)?;
                }
                solid.moved(&|p| p + by)?
            }
            Motion::Rotate => {
                let axis = args.axis.as_ref().context("pick an axis to turn about")?;
                let axis = builder.tracked().resolve(axis)?;
                let angle = (*builder).measure(&args.angle, "the angle")?.to_radians();
                solid.moved(&|p| turned(&axis, angle, p))?
            }
            Motion::Mirror => {
                let plane = args.plane.as_ref().context("pick a plane to mirror in")?;
                let plane = builder.tracked().resolve(plane)?;
                let (o, n) = (plane.origin(), plane.normal());
                solid.mapped(&|p| p - n * (2.0 * (p - o).dot(n)), true, 1.0)?
            }
            Motion::Scale => {
                let factor = (*builder).measure(&args.factor, "the factor")?;
                ensure!(
                    factor > 0.0 && factor.is_finite(),
                    "a body is scaled by more than nothing"
                );
                solid.mapped(&|p| p * factor, false, factor)?
            }
        };
        let topo = &builder.tracked().topo;
        let called = if args.copy {
            self.id.clone()
        } else {
            out.remove(topo, body);
            name
        };
        if sheet {
            out.write_sheet(&made, &called);
        } else {
            out.write(&made, &called);
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let args = &self.args;
        let of = args.body.as_ref().map_or("(not picked)", |s| s.name());
        let what = match args.motion {
            Motion::Move => format!("by ({}, {}, {})", args.by[0], args.by[1], args.by[2]),
            Motion::Rotate => format!("turned {}°", args.angle),
            Motion::Mirror => "mirrored".to_owned(),
            Motion::Scale => format!("scaled {}×", args.factor),
        };
        format!("{}{of} {what}", if args.copy { "a copy of " } else { "" })
    }
}

/// What is repeated along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Along {
    /// A line: one copy every so far along an axis.
    #[default]
    Line,
    /// A circle: the copies spread evenly around an axis.
    Circle,
    /// A path: the copies spread evenly along the curves of a sketch,
    /// from the end the body is at.
    Path,
    /// The points of a sketch: a copy at each, moved there from where
    /// what is repeated is (no copy at a point where it is already).
    Points,
    /// A face, filled: a copy at each place of a grid or of rings laid
    /// out from where what is repeated is, as many as fit inside it.
    Fill,
}

/// `solid` at each place of a pattern after the first: `count − 1`
/// copies, each `spacing` further along `axis`, or turned about it by an
/// even share of `angle` (radians; a full turn is shared by all, less
/// than that reaches from the first to the last).
pub fn repeated(
    solid: &Solid,
    along: Along,
    axis: &DatumAxis,
    count: usize,
    spacing: f64,
    angle: f64,
) -> anyhow::Result<Vec<Solid>> {
    let mut out = Vec::new();
    let full = angle >= std::f64::consts::TAU - 1e-12;
    for k in 1..count {
        let made = match along {
            Along::Line => {
                let by = axis.direction() * (spacing * k as f64);
                solid.moved(&|p| p + by)?
            }
            Along::Circle => {
                let shares = if full { count } else { count - 1 };
                let turn = angle.min(std::f64::consts::TAU) * k as f64 / shares as f64;
                solid.moved(&|p| turned(axis, turn, p))?
            }
            Along::Path | Along::Points | Along::Fill => {
                anyhow::bail!("a sketch or a face says where these go")
            }
        };
        out.push(made);
    }
    Ok(out)
}

/// Where the copies of a pattern go: along a line, around an axis,
/// along a path, at the points of a sketch or filling a face.
pub(crate) struct Places<'a> {
    pub(crate) along: Along,
    pub(crate) axis: &'a Option<Reference<DatumAxis>>,
    pub(crate) sketch: &'a Option<workpiece_core::Reference<workpiece_core::Sketched>>,
    pub(crate) count: usize,
    pub(crate) spacing: &'a Measure,
    pub(crate) angle: &'a Measure,
    pub(crate) upright: bool,
    /// Filling: the face, how, the rows' spacing, their angle, the
    /// polygon's sides and the margin.
    pub(crate) face: &'a Option<Reference<FaceId>>,
    pub(crate) grid: Grid,
    pub(crate) across: &'a Measure,
    pub(crate) turned: &'a Measure,
    pub(crate) sides: usize,
    pub(crate) margin: &'a Measure,
}

impl Places<'_> {
    /// The numbers the pattern spreads its copies by, said of the
    /// model by the step `id` where the first copy goes from what is
    /// repeated (about `near`): along a line how far apart they are,
    /// around a circle short of all the way round the angle they are
    /// spread over. Nothing where they go along a sketch.
    pub(crate) fn said(
        &self,
        builder: &dyn Builder,
        id: &str,
        near: Point3,
    ) -> Vec<(String, workpiece_core::world::Annotation)> {
        use workpiece_core::world::{Annotation, Measured};
        let Some(Ok(axis)) = self.axis.as_ref().map(|a| builder.tracked().resolve(a)) else {
            return Vec::new();
        };
        let (o, d) = (axis.origin(), axis.direction());
        match self.along {
            Along::Line => {
                let Ok(spacing) = (*builder).measure(self.spacing, "the spacing") else {
                    return Vec::new();
                };
                let measured = Measured::Length {
                    from: near,
                    to: near + d * spacing,
                    along: None,
                };
                let held = Annotation::held(measured, spacing.abs(), self.spacing, 0.0);
                vec![(format!("{id}/spacing"), held)]
            }
            Along::Circle => {
                let Ok(angle) = (*builder).measure(self.angle, "the angle") else {
                    return Vec::new();
                };
                let r = near - o;
                let at = o + d * r.dot(d);
                let Some(first) = (near - at).try_normalize() else {
                    return Vec::new();
                };
                if angle <= 0.0 || angle >= 360.0 - 1e-9 {
                    return Vec::new();
                }
                let turned = turned(&axis, angle.to_radians(), at + first) - at;
                let measured = Measured::Angle {
                    at,
                    first,
                    second: turned,
                };
                let reach = near.distance(at);
                let held = Annotation::held(measured, angle, self.angle, reach);
                vec![(format!("{id}/angle"), held)]
            }
            Along::Path | Along::Points | Along::Fill => Vec::new(),
        }
    }
}

/// `solid` at each place of the pattern `args` after the first (where
/// what is repeated is about `near`; `sized` is what is repeated, which
/// says how far it reaches: of a fill, how far in from the face's edges
/// it has to be).
pub(crate) fn places(
    args: &Places<'_>,
    builder: &dyn Builder,
    solid: &Solid,
    near: Point3,
    sized: &Solid,
) -> anyhow::Result<Vec<Solid>> {
    let count = args.count;
    let counted = || -> anyhow::Result<()> {
        ensure!((2..=10_000).contains(&count), "a pattern is of two or more");
        Ok(())
    };
    let spacing = (*builder).measure(args.spacing, "the spacing")?;
    let angle = (*builder).measure(args.angle, "the angle")?.to_radians();
    Ok(match args.along {
        Along::Line | Along::Circle => {
            counted()?;
            let axis = args.axis.as_ref().context("pick an axis")?;
            let axis = builder.tracked().resolve(axis)?;
            match args.along {
                Along::Line => ensure!(spacing != 0.0, "the copies are some way apart"),
                _ => ensure!(angle > 0.0, "the copies are spread over some angle"),
            }
            repeated(solid, args.along, &axis, count, spacing, angle)?
        }
        Along::Path => {
            counted()?;
            let sketch = args
                .sketch
                .as_ref()
                .context("pick the sketch of the path")?;

            let (curves, closes, normal) =
                workpiece_shape::paths::path(builder.tracked(), sketch, near)?;
            along_curves(solid, &curves, closes, normal, count, args.upright)?
        }
        Along::Points => {
            let sketch = args
                .sketch
                .as_ref()
                .context("pick the sketch of the points")?;
            let name = sketch.name();
            let profile = builder
                .tracked()
                .profiles
                .get(name)
                .with_context(|| format!("there is no sketch {name:?}"))?;
            let moves = to_points(profile, near, reach(sized, near, profile.plane.normal()));
            ensure!(
                !moves.is_empty(),
                "the sketch {name:?} has no points but where what is repeated is: put one where each copy goes"
            );
            let mut copies = Vec::new();
            for by in moves {
                copies.push(solid.moved(&|p| p + by)?);
            }
            copies
        }
        Along::Fill => {
            let face = args.face.as_ref().context("pick the face to fill")?;
            let world = builder.tracked();
            let plane = <DatumPlane as workpiece_core::Resolve>::resolve(world, &face.source)
                .map_err(|e| anyhow::anyhow!("{e}: the face filled is flat"))?;
            let id = world
                .topo
                .resolve::<FaceId>(face.name())
                .with_context(|| format!("there is no face {:?}", face.name()))?;
            let outline = crate::fill::outline(&world.topo, id, &plane)?;
            let local = |p: Point3| {
                let d = p - plane.origin();
                Point2::new(d.dot(plane.x_dir()), d.dot(plane.y_dir()))
            };
            // The rows run along the direction picked, as it lies in
            // the face, or along the face's own x; turned by the angle.
            let base = match args.axis.as_ref() {
                Some(axis) => {
                    let d = builder.tracked().resolve(axis)?.direction();
                    let (x, y) = (d.dot(plane.x_dir()), d.dot(plane.y_dir()));
                    ensure!(
                        libm::hypot(x, y) > 1e-9,
                        "the direction stands across the face: pick one along it"
                    );
                    libm::atan2(y, x)
                }
                None => 0.0,
            };
            let layout = crate::fill::Layout {
                grid: args.grid,
                spacing,
                across: (*builder).measure(args.across, "the rows' spacing")?,
                angle: base
                    + (*builder)
                        .measure(args.turned, "the rows' angle")?
                        .to_radians(),
                sides: args.sides,
                margin: (*builder).measure(args.margin, "the margin")?,
                reach: reach(sized, near, plane.normal()),
            };
            let seed = local(near);
            let mut copies = Vec::new();
            for q in crate::fill::places(&layout, &outline, seed)? {
                let by = plane.x_dir() * (q.x - seed.x) + plane.y_dir() * (q.y - seed.y);
                copies.push(solid.moved(&|p| p + by)?);
            }
            copies
        }
    })
}

/// How far `solid` reaches from `near` across `normal`: from the line
/// through `near` along it, the furthest of its corners.
fn reach(solid: &Solid, near: Point3, normal: Point3) -> f64 {
    let n = normal.try_normalize().unwrap_or(Point3::Z);
    solid
        .vertices
        .iter()
        .map(|v| (v.point - near).reject_from_normalized(n).length())
        .fold(0.0, f64::max)
}

/// The moves that take what is repeated (about `near`, reaching
/// `reach` across the sketch's plane) to the points of `profile` that
/// are no curve's end. Where it is at one of them already (one within
/// its reach), it is moved from that point exactly and none goes
/// there; else it is moved from where it is, seen square onto the
/// sketch's plane.
pub(crate) fn to_points(
    profile: &workpiece_core::world::Profile,
    near: Point3,
    reach: f64,
) -> Vec<Point3> {
    let mut used = vec![false; profile.vertices.len()];
    for edge in &profile.edges {
        used[edge.start] = true;
        used[edge.end] = true;
    }
    let plane = profile.plane;
    let place = |p: workpiece_core::math::Point2| {
        plane.origin() + plane.x_dir() * p.x + plane.y_dir() * p.y
    };
    let points: Vec<Point3> = (0..used.len())
        .filter(|v| !used[*v])
        .map(|v| place(profile.vertices[v].point))
        .collect();
    let n = plane.normal();
    let foot = near - n * (near - plane.origin()).dot(n);
    let nearest = (0..points.len()).min_by(|a, b| {
        points[*a]
            .distance(foot)
            .total_cmp(&points[*b].distance(foot))
    });
    let close = reach.max(1e-6 * (1.0 + foot.length()));
    let at = nearest.filter(|k| points[*k].distance(foot) <= close);
    let from = at.map_or(foot, |k| points[k]);
    points
        .iter()
        .enumerate()
        .filter(|(k, _)| Some(*k) != at)
        .map(|(_, p)| *p - from)
        .collect()
}

/// About where `solid` is: the middle of the box around it — on the
/// axis of a bore, the middle of a boss.
pub(crate) fn whereabouts(solid: &Solid) -> Point3 {
    if solid.faces.is_empty() {
        return Point3::ZERO;
    }
    let (lo, hi) = solid.extent();
    (lo + hi) / 2.0
}

/// `solid` at `count − 1` more places spread evenly along `path` (curves
/// each starting where the one before ends) from its start: all the way
/// around, if it `closes`, else to its end. Each is turned about
/// `normal` as the path has turned, unless they stay `upright`.
pub fn along_curves(
    solid: &Solid,
    path: &[Curve3],
    closes: bool,
    normal: Point3,
    count: usize,
    upright: bool,
) -> anyhow::Result<Vec<Solid>> {
    // Where along the path a length is: by its sampled lengths.
    const STEPS: usize = 256;
    let mut marks = vec![(0.0, 0, 0.0)];
    let mut whole = 0.0;
    for (k, curve) in path.iter().enumerate() {
        let (a, b) = curve.domain();
        for i in 0..STEPS {
            let at = |i: usize| a + (b - a) * i as f64 / STEPS as f64;
            whole += curve.point_at(at(i)).distance(curve.point_at(at(i + 1)));
            marks.push((whole, k, at(i + 1)));
        }
    }
    ensure!(whole > 0.0, "a path has some length");
    let place = |length: f64| {
        let i = marks
            .partition_point(|m| m.0 < length)
            .clamp(1, marks.len() - 1);
        let (before, after) = (marks[i - 1], marks[i]);
        let curve = &path[after.1];
        let from = if before.1 == after.1 {
            before.2
        } else {
            curve.domain().0
        };
        let within = ((length - before.0) / (after.0 - before.0).max(1e-300)).clamp(0.0, 1.0);
        curve.derivative_at(from + (after.2 - from) * within)
    };
    let normal = normal
        .try_normalize()
        .context("the path's plane has a normal")?;
    let (origin, way) = place(0.0);
    let shares = if closes { count } else { count - 1 };
    let mut out = Vec::new();
    for k in 1..count {
        let (at, onward) = place(whole * k as f64 / shares as f64);
        // About the normal, from the way the path set out to the way
        // it goes here.
        let angle = if upright {
            0.0
        } else {
            let (a, b) = (way.reject_from(normal), onward.reject_from(normal));
            let angle = angle_between(a, b) * a.cross(b).dot(normal).signum();
            if angle.is_finite() { angle } else { 0.0 }
        };
        let (sin, cos) = libm::sincos(angle);
        let turn =
            |v: Point3| v * cos + normal.cross(v) * sin + normal * (normal.dot(v) * (1.0 - cos));
        out.push(solid.moved(&|p| at + turn(p - origin))?);
    }
    Ok(out)
}

/// `solid` as the instance `called` of a pattern: all that has a name
/// in it named under the instance's.
pub(crate) fn instance(mut solid: Solid, called: &str) -> Solid {
    let under = |name: &mut Option<String>| {
        if let Some(name) = name {
            *name = format!("{called}/{name}");
        }
    };
    solid.vertices.iter_mut().for_each(|v| under(&mut v.name));
    for edge in &mut solid.edges {
        under(&mut edge.name);
        edge.also = edge.also.iter().map(|n| format!("{called}/{n}")).collect();
    }
    for face in &mut solid.faces {
        under(&mut face.name);
        face.also = face.also.iter().map(|n| format!("{called}/{n}")).collect();
    }
    solid
}

impl workpiece_core::Choices for Motion {
    const ALL: &'static [Self] = &MOTIONS;

    fn label(self) -> &'static str {
        Self::label(self)
    }

    fn icon(self) -> Option<&'static str> {
        Some(match self {
            Motion::Move => "motion-move",
            Motion::Rotate => "motion-rotate",
            Motion::Mirror => "motion-mirror",
            Motion::Scale => "motion-scale",
        })
    }
}

impl workpiece_core::Choices for Along {
    const ALL: &'static [Self] = &[
        Along::Line,
        Along::Circle,
        Along::Path,
        Along::Points,
        Along::Fill,
    ];

    fn label(self) -> &'static str {
        match self {
            Along::Line => "Line",
            Along::Circle => "Circle",
            Along::Path => "Path",
            Along::Points => "Points",
            Along::Fill => "Fill",
        }
    }

    fn icon(self) -> Option<&'static str> {
        Some(match self {
            Along::Line => "along-line",
            Along::Circle => "along-circle",
            Along::Path => "along-path",
            Along::Points => "along-points",
            Along::Fill => "along-fill",
        })
    }
}

/// The solid body of `topo` called `name`.
pub(crate) fn named_solid(topo: &Topo, name: &str) -> Option<BodyId> {
    let body = topo.body_called(name)?;
    (topo.body(body)?.kind == workpiece_core::world::topo::BodyKind::Solid).then_some(body)
}

/// A whole number, read from one written with a fraction too (as counts
/// once were): rounded.
pub(crate) fn whole<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<usize, D::Error> {
    let number = <f64 as serde::Deserialize>::deserialize(deserializer)?;
    Ok(number.round().max(0.0) as usize)
}
