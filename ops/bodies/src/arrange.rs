//! Transform and Pattern: a body moved, turned, mirrored or scaled (or a
//! copy of it), and a body repeated along a line or around an axis.

use anyhow::{Context, ensure};
use workpiece_boolean::{Op, Solid, combine, read};
use workpiece_core::math::Point3;
use workpiece_core::world::topo::{BodyId, Curve3, Topo};
use workpiece_core::world::{DatumAxis, DatumPlane};
use workpiece_core::{Builder, Measure, Operation, Reference};

use workpiece_shape::bodies::Combine;
use workpiece_shape::ui;

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
    o + along + across * angle.cos() + d.cross(across) * angle.sin()
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
    #[arg(label = "Keep", options = "Move the body|Make a copy")]
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

/// What a body is repeated along.
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
    /// The points of a sketch: a copy at each, as the body is at the
    /// one nearest it.
    Points,
}

/// The body, what it is repeated along, and what becomes of the copies.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct PatternArgs {
    /// The body that is repeated.
    pub body: Option<workpiece_core::Reference<workpiece_core::world::topo::BodyId>>,
    /// Along a line or around a circle.
    #[arg(choice)]
    pub along: Along,
    /// The axis: the line's direction, or what the circle is around.
    #[arg(when = "matches!(self.along, Along::Line | Along::Circle)")]
    pub axis: Option<Reference<DatumAxis>>,
    /// The sketch whose curves are the path, or whose points are the
    /// places.
    #[serde(default)]
    #[arg(when = "matches!(self.along, Along::Path | Along::Points)")]
    pub sketch: Option<workpiece_core::Reference<workpiece_core::Sketched>>,
    /// How many there are in the end, the body itself counted.
    #[serde(deserialize_with = "whole")]
    #[arg(when = "self.along != Along::Points")]
    pub count: usize,
    /// Along a line: how far each is from the one before.
    #[arg(when = "self.along == Along::Line")]
    pub spacing: Measure,
    /// Around a circle: the angle they are spread over, in degrees (360
    /// or more: all the way around).
    #[arg(label = "Angle °", when = "self.along == Along::Circle")]
    pub angle: Measure,
    /// Along a path: whether the copies keep facing the way the body
    /// does (else each is turned as the path has turned).
    #[serde(default)]
    #[arg(
        label = "Copies",
        options = "Turn with the path|Stay as the body is",
        when = "self.along == Along::Path"
    )]
    pub upright: bool,
    /// What becomes of them: bodies of their own, one body with the one
    /// repeated, or cut out of (or kept in common with) the target.
    #[arg(choice, label = "Operation")]
    pub combine: Combine,
    /// The body they are cut out of.
    #[arg(
        label = "Of",
        when = "matches!(self.combine, Combine::Cut | Combine::Intersect)"
    )]
    pub target: Option<workpiece_core::Reference<workpiece_core::world::topo::BodyId>>,
}

impl Default for PatternArgs {
    fn default() -> Self {
        Self {
            body: None,
            along: Along::Line,
            axis: None,
            count: 3,
            spacing: Measure::new(10.0),
            angle: Measure::new(360.0),
            sketch: None,
            upright: false,
            combine: Combine::NewBody,
            target: None,
        }
    }
}

/// Repeats a body along a line, around an axis, along the curves of a
/// sketch or at its points. The copies are bodies
/// of their own (`{id}/1`, `{id}/2`, …), or joined with the body into
/// one, or — all of them, the body too — cut out of another body.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pattern {
    /// The operation's id.
    pub id: String,
    /// What is repeated, and how.
    pub args: PatternArgs,
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
            Along::Path | Along::Points => anyhow::bail!("a sketch says where these go"),
        };
        out.push(made);
    }
    Ok(out)
}

/// Where the copies of a pattern go: along a line, around an axis,
/// along a path or at the points of a sketch.
pub(crate) struct Places<'a> {
    pub(crate) along: Along,
    pub(crate) axis: &'a Option<Reference<DatumAxis>>,
    pub(crate) sketch: &'a Option<workpiece_core::Reference<workpiece_core::Sketched>>,
    pub(crate) count: usize,
    pub(crate) spacing: &'a Measure,
    pub(crate) angle: &'a Measure,
    pub(crate) upright: bool,
}

impl<'a> Places<'a> {
    fn of(args: &'a PatternArgs) -> Self {
        Self {
            along: args.along,
            axis: &args.axis,
            sketch: &args.sketch,
            count: args.count,
            spacing: &args.spacing,
            angle: &args.angle,
            upright: args.upright,
        }
    }
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
            Along::Path | Along::Points => Vec::new(),
        }
    }
}

/// `solid` at each place of the pattern `args` after the first (where
/// it is, about `near`).
pub(crate) fn places(
    args: &Places<'_>,
    builder: &dyn Builder,
    solid: &Solid,
    near: Point3,
) -> anyhow::Result<Vec<Solid>> {
    let count = args.count;
    ensure!((2..=10_000).contains(&count), "a pattern is of two or more");
    let spacing = (*builder).measure(args.spacing, "the spacing")?;
    let angle = (*builder).measure(args.angle, "the angle")?.to_radians();
    Ok(match args.along {
        Along::Line | Along::Circle => {
            let axis = args.axis.as_ref().context("pick an axis")?;
            let axis = builder.tracked().resolve(axis)?;
            match args.along {
                Along::Line => ensure!(spacing != 0.0, "the copies are some way apart"),
                _ => ensure!(angle > 0.0, "the copies are spread over some angle"),
            }
            repeated(solid, args.along, &axis, count, spacing, angle)?
        }
        Along::Path => {
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
            let world = builder.tracked();
            let profile = world
                .profiles
                .get(name)
                .with_context(|| format!("there is no sketch {name:?}"))?;
            // The points that are no curve's end; the body is at the
            // one nearest it.
            let mut used = vec![false; profile.vertices.len()];
            for edge in &profile.edges {
                used[edge.start] = true;
                used[edge.end] = true;
            }
            let plane = profile.plane;
            let place = |p: workpiece_core::math::Point2| {
                plane.origin() + plane.x_dir() * p.x + plane.y_dir() * p.y
            };
            let mut places: Vec<Point3> = (0..used.len())
                .filter(|v| !used[*v])
                .map(|v| place(profile.vertices[v].point))
                .collect();
            ensure!(
                places.len() >= 2,
                "the sketch {name:?} has fewer than two points: one where the body is, one for each copy"
            );

            let first = (0..places.len())
                .min_by(|a, b| {
                    places[*a]
                        .distance(near)
                        .total_cmp(&places[*b].distance(near))
                })
                .unwrap_or(0);
            let from = places.remove(first);
            let mut copies = Vec::new();
            for to in places {
                copies.push(solid.moved(&|p| p + (to - from))?);
            }
            copies
        }
    })
}

/// About where `solid` is: the middle of its corners (or, of a body
/// without any, of what its faces are made of).
pub(crate) fn whereabouts(solid: &Solid) -> Point3 {
    let corners: Vec<Point3> = if solid.vertices.is_empty() {
        let faces = solid.faces.iter();
        faces
            .flat_map(|f| f.surface.control_points().to_vec())
            .collect()
    } else {
        solid.vertices.iter().map(|v| v.point).collect()
    };
    corners.iter().sum::<Point3>() / corners.len().max(1) as f64
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
            let angle = a.angle_between(b) * a.cross(b).dot(normal).signum();
            if angle.is_finite() { angle } else { 0.0 }
        };
        let (sin, cos) = angle.sin_cos();
        let turn =
            |v: Point3| v * cos + normal.cross(v) * sin + normal * (normal.dot(v) * (1.0 - cos));
        out.push(solid.moved(&|p| at + turn(p - origin))?);
    }
    Ok(out)
}

impl Operation for Pattern {
    type Args = PatternArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &PatternArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut PatternArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = workpiece_shape::bodies::Out::default();
        let args = &self.args;
        let (body, name) = body_of(&builder.tracked().topo, &args.body, "the body to repeat")?;
        let solid = read(&builder.tracked().topo, body)?;
        let near = whereabouts(&solid);
        let copies = places(&Places::of(args), builder, &solid, near)?;
        // Each copy's faces, edges and vertices are called after the
        // copy (`pattern1/2/…`): what refers to one of an instance
        // finds it again, whatever the other instances are.
        let copies: Vec<Solid> = copies
            .into_iter()
            .enumerate()
            .map(|(k, copy)| instance(copy, &format!("{}/{}", self.id, k + 1)))
            .collect();
        let topo = &builder.tracked().topo;
        match args.combine {
            Combine::NewBody => {
                for (k, copy) in copies.iter().enumerate() {
                    out.write(copy, &format!("{}/{}", self.id, k + 1));
                }
            }
            Combine::Join => {
                let mut all = solid;
                for (k, copy) in copies.iter().enumerate() {
                    all = combine(Op::Union, &all, copy)
                        .with_context(|| format!("joining copy {}", k + 1))?
                        .solid;
                }
                out.remove(topo, body);
                out.write(&all, &name);
            }
            Combine::Cut | Combine::Intersect => {
                let (target, target_name) = body_of(topo, &args.target, "the body to cut")?;
                ensure!(target != body, "a body is not cut out of itself");
                let mut result = read(topo, target)?;
                if args.combine == Combine::Cut {
                    for (k, tool) in std::iter::once(&solid).chain(&copies).enumerate() {
                        result = combine(Op::Cut, &result, tool)
                            .with_context(|| format!("cutting out number {}", k + 1))?
                            .solid;
                    }
                } else {
                    // What the target shares with any of them.
                    let mut all = solid;
                    for copy in &copies {
                        all = combine(Op::Union, &all, copy)?.solid;
                    }
                    result = combine(Op::Intersect, &result, &all)?.solid;
                }
                out.remove(topo, target);
                out.remove(topo, body);
                if !result.is_empty() {
                    out.write(&result, &target_name);
                }
            }
        }
        out.made.annotations = Places::of(args).said(builder, &self.id, near);
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let args = &self.args;
        let of = args.body.as_ref().map_or("(not picked)", |s| s.name());
        let how = match args.along {
            Along::Line => format!("{} apart", args.spacing),
            Along::Circle => format!("around {}°", args.angle),
            Along::Path => "along a path".to_owned(),
            Along::Points => "at the points of a sketch".to_owned(),
        };
        format!(
            "{} of {of}, {how}, {}",
            args.count,
            args.combine.label().to_lowercase()
        )
    }
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
}

impl workpiece_core::Choices for Along {
    const ALL: &'static [Self] = &[Along::Line, Along::Circle, Along::Path, Along::Points];

    fn label(self) -> &'static str {
        match self {
            Along::Line => "A line",
            Along::Circle => "A circle",
            Along::Path => "A path",
            Along::Points => "Points",
        }
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
