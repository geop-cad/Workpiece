//! Boundary: a surface over a hole of any number of sides, each side
//! an edge or a curve, the surface running on from the face beside it
//! smoothly (G1, G2) where that is asked.

use std::sync::Arc;

use anyhow::{Context, ensure};
use std::collections::BTreeMap;
use workpiece_boolean::model::Walk;
use workpiece_boolean::{Solid, read, read_sheet};
use workpiece_core::math::Point3;

use workpiece_core::handles::PlanarHandle;
use workpiece_core::widgets::{
    Choice, HBox, Label, Mood, NumericInput, ReferenceInput, ReferenceList, ToolButton, VBox,
};
use workpiece_core::world::topo::{BodyId, BodyKind, Curve3, EdgeId, FaceId, Topo};
use workpiece_core::{
    Anything, Builder, Edit, EditContext, EditVisualization, Hotkey, Operation, Reference,
    ReferenceKind, Source, Widget,
};
use workpiece_shape::fill::{Across, Side, filled_over};

/// How a surface runs on from the face beside one of its sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Continuity {
    /// Only through the side: a crease along it.
    #[default]
    Free,
    /// Tangent to the face beside it (G1): no crease.
    Tangent,
    /// Tangent, and bending as the face does across the side (G2).
    Curvature,
}

impl workpiece_core::arguments::Choices for Continuity {
    const ALL: &'static [Self] = &[Self::Free, Self::Tangent, Self::Curvature];

    fn label(self) -> &'static str {
        match self {
            Self::Free => "Free (G0)",
            Self::Tangent => "Tangent (G1)",
            Self::Curvature => "Curvature (G2)",
        }
    }

    fn icon(self) -> Option<&'static str> {
        Some(match self {
            Self::Free => "end-free",
            Self::Tangent => "end-tangent",
            Self::Curvature => "end-curvature",
        })
    }
}

/// One side of a boundary surface.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct BoundarySide {
    /// The edge (of a surface or a solid) or the curve (a curve body
    /// of one edge) it goes along.
    pub edge: Reference<Anything>,
    /// How the surface runs on from the face beside it.
    #[serde(default)]
    #[arg(hidden)]
    pub way: Continuity,
    /// The face it runs on from; none for the face the edge bounds (of
    /// a surface's free edge, its one face).
    #[serde(default)]
    #[arg(hidden)]
    pub face: Option<Reference<FaceId>>,
    /// Whether it leaves the other way: back over the face beside it
    /// (G2: bending as the face does, the other way).
    #[serde(default)]
    #[arg(hidden)]
    pub flip: bool,
    /// How full it leaves the side: the length of the way it leaves, as
    /// a share of its natural one (1).
    #[serde(default = "one")]
    #[arg(hidden)]
    pub magnitude: f64,
    /// The way it leaves within the tangent plane of the face beside it,
    /// in degrees from square across the side (towards the way the side
    /// runs): 0 leaves square to it.
    #[serde(default)]
    #[arg(hidden)]
    pub angle: f64,
}

fn one() -> f64 {
    1.0
}

impl BoundarySide {
    /// A side along `edge`, free.
    pub fn free(edge: Reference<Anything>) -> Self {
        Self {
            edge,
            way: Continuity::Free,
            face: None,
            flip: false,
            magnitude: 1.0,
            angle: 0.0,
        }
    }
}

/// What a boundary surface goes around.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    workpiece_core::Arguments,
)]
pub struct BoundaryArgs {
    /// Its sides: edges or curves that meet end to end around a hole,
    /// in any order.
    #[arg(nested, with = "sides_field")]
    pub sides: Vec<BoundarySide>,
}

/// Adds a surface over the hole its sides go around (flat where they
/// all lie in a plane and are free), its face `{id}/face` and its edges
/// `{id}/e0`, … as the sides are given: sewn to the surfaces whose free
/// edges it runs along — one surface with them, under the first one's
/// name, or a solid where that closes it all around — or else a surface
/// body named after the operation's id.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Boundary {
    /// The operation's id; also the body's name.
    pub id: String,
    /// Its sides.
    pub args: BoundaryArgs,
}

/// The body (read) that has the edge or face called `name`, and the
/// body's name.
fn body_with(topo: &Topo, name: &str) -> anyhow::Result<Solid> {
    let id = topo
        .referred(name)
        .with_context(|| format!("there is nothing called {name:?}"))?;
    let body = topo
        .owner(id)
        .with_context(|| format!("{name:?} is of no body"))?;
    Ok(match topo.body(body).map(|b| b.kind) {
        Some(BodyKind::Sheet) => read_sheet(topo, body)?,
        Some(BodyKind::Solid) => read(topo, body)?,
        _ => anyhow::bail!("{name:?} is not of a surface or a solid"),
    })
}

/// Whether `edge` of a solid read is the one called `name`.
fn called(edge: &workpiece_boolean::model::Edge, name: &str) -> bool {
    edge.name.as_deref() == Some(name) || edge.also.iter().any(|n| n == name)
}

/// The curve of the edge or one-edged curve `source`, and the part of
/// it that counts.
fn curve_of(topo: &Topo, source: &Source) -> anyhow::Result<(Arc<Curve3>, (f64, f64))> {
    let Source::Topology(name) = source else {
        anyhow::bail!("a side is an edge or a curve: {} is neither", source.name());
    };
    // A curve body: its one edge.
    let edge = match topo.resolve::<EdgeId>(name) {
        Some(edge) => edge,
        None => {
            let body = topo
                .resolve(name)
                .and_then(|b| topo.body(b))
                .with_context(|| format!("there is no edge or curve {name:?}"))?;
            ensure!(
                body.kind == BodyKind::Wire && body.free_edges.len() == 1,
                "{name:?} is no curve of one edge: pick its edges one by one"
            );
            body.free_edges[0]
        }
    };
    let edge = topo.edge(edge).context("an edge without its curve")?;
    let curve = topo
        .curve(edge.curve)
        .context("an edge without its curve")?;
    Ok((Arc::new(curve.clone()), (edge.t0, edge.t1)))
}

/// How the surface leaves the side along edge `name`, from face `face`
/// of `solid` (whose loops walk it): at the edge's parameter `t`.
fn across_from(solid: &Solid, face: usize, edge: usize, t: f64, side: &BoundarySide) -> Across {
    let it = &solid.faces[face];
    let walked = it
        .loops
        .iter()
        .flatten()
        .find(|u| matches!(u.walk, Walk::Edge { edge: e, .. } if e == edge));
    let curve = &solid.edges[edge].curve;
    let (p, tangent) = curve.derivative_at(t);
    let (uv, reversed) = match walked {
        Some(u) => (
            u.pcurve.point_at(t),
            matches!(u.walk, Walk::Edge { reversed: true, .. }),
        ),
        None => (
            workpiece_boolean::geometry::foot(&it.surface, p, Default::default()).0,
            false,
        ),
    };
    let surface = &it.surface;
    let (_, du, dv) = surface.derivatives_at(uv.x, uv.y);
    let normal = du.cross(dv).normalize_or_zero();
    let along = if reversed { -tangent } else { tangent }.normalize_or_zero();
    // The face is on the left of its loops, its normal up: away from it
    // is to the right; turned as the side says, within the face's
    // tangent plane.
    let square = along.cross(normal).normalize_or_zero();
    let away = turned_in(square, normal, side.angle);
    // How it bends that way: a little step into the face, and another.
    let (a, b, c) = (du.dot(du), du.dot(dv), dv.dot(dv));
    let det = a * c - b * b;
    let (r, s) = (away.dot(du), away.dot(dv));
    let step = if det.abs() > 1e-300 {
        workpiece_core::math::Point2::new((r * c - s * b) / det, (s * a - r * b) / det)
    } else {
        workpiece_core::math::Point2::ZERO
    };
    let ((u0, u1), (v0, v1)) = surface.domain();
    let e = 1e-4 * ((u1 - u0).abs() + (v1 - v0).abs()) / (step.length().max(1e-300) * 2.0);
    let at = |k: f64| {
        let q = uv - step * (e * k);
        surface.point_at(q.x.clamp(u0, u1), q.y.clamp(v0, v1))
    };
    let second = (at(0.0) - at(1.0) * 2.0 + at(2.0)) / (e * e);
    // (As full as asked: the way it leaves that much longer, its bend
    // as much again; flipped, the other way.)
    let m = side.magnitude;
    let sign = if side.flip { -1.0 } else { 1.0 };
    Across {
        away: away * (m * sign),
        normal,
        bend: second.dot(normal) * m * m,
    }
}

/// `square` (a direction in the plane of `normal`) turned `degrees`
/// about `normal`.
fn turned_in(square: Point3, normal: Point3, degrees: f64) -> Point3 {
    let a = degrees.to_radians();
    square * libm::cos(a) + normal.cross(square) * libm::sin(a)
}

/// The face a side runs on from — read with its body — and which edge
/// of that the side is; none for a free side.
type Beside = Option<(Solid, usize, usize)>;

/// What `side` runs on from, in `topo`.
fn beside(topo: &Topo, side: &BoundarySide) -> anyhow::Result<Beside> {
    if side.way == Continuity::Free {
        return Ok(None);
    }
    let name = side.edge.name();
    let solid = match &side.face {
        Some(face) => body_with(topo, face.name())?,
        None => body_with(topo, name)
            .with_context(|| format!("{name:?} is of no face to run on from: pick one"))?,
    };
    let edge = solid
        .edges
        .iter()
        .position(|e| called(e, name))
        .with_context(|| format!("the face is not beside {name:?}"))?;
    let walks = |f: &workpiece_boolean::model::Face| {
        f.loops
            .iter()
            .flatten()
            .any(|u| matches!(u.walk, Walk::Edge { edge: e, .. } if e == edge))
    };
    let face = match &side.face {
        Some(face) => solid
            .faces
            .iter()
            .position(|f| {
                f.name.as_deref() == Some(face.name()) || f.also.iter().any(|n| n == face.name())
            })
            .filter(|&f| walks(&solid.faces[f]))
            .with_context(|| format!("{:?} is not beside {name:?}", face.name()))?,
        None => {
            let faces: Vec<usize> = (0..solid.faces.len())
                .filter(|&f| walks(&solid.faces[f]))
                .collect();
            ensure!(
                faces.len() == 1,
                "{name:?} is between two faces: pick the one to run on from"
            );
            faces[0]
        }
    };
    Ok(Some((solid, face, edge)))
}

impl Operation for Boundary {
    type Args = BoundaryArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &BoundaryArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut BoundaryArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let topo = &builder.tracked().topo;
        let sides = &self.args.sides;
        ensure!(
            !sides.is_empty(),
            "pick the edges around the hole, meeting end to end"
        );
        // Each side's curve, and the face beside it with which of its
        // edges the side is, where it runs on from one.
        // (Of each: its curve, the part of it, and the face beside it
        // — read with its body — and which edge of that the side is.)
        let mut held: Vec<(Arc<Curve3>, (f64, f64), Beside)> = Vec::new();
        for side in sides {
            let (curve, range) = curve_of(topo, &side.edge.source)?;
            held.push((curve, range, beside(topo, side)?));
        }
        let fns: Vec<Box<dyn Fn(f64) -> Across + '_>> = held
            .iter()
            .zip(sides)
            .map(
                |((_, _, beside), side)| -> Box<dyn Fn(f64) -> Across + '_> {
                    match beside {
                        Some((solid, face, edge)) => {
                            Box::new(move |t| across_from(solid, *face, *edge, t, side))
                        }
                        None => Box::new(|_| Across {
                            away: Point3::ZERO,
                            normal: Point3::ZERO,
                            bend: 0.0,
                        }),
                    }
                },
            )
            .collect();
        let made: Vec<Side<'_>> = held
            .iter()
            .zip(sides)
            .zip(&fns)
            .enumerate()
            .map(|(k, (((curve, range, _), side), f))| Side {
                curve: curve.clone(),
                range: *range,
                across: (side.way != Continuity::Free)
                    .then(|| (&**f, side.way == Continuity::Curvature)),
                name: Some(format!("{}/e{k}", self.id)),
            })
            .collect();
        let free = made.iter().all(|s| s.across.is_none());
        // All free and in one plane: a flat face. Four sides, all free:
        // the patch blended from them (Coons), which runs along them
        // exactly.
        let flat = if free {
            workpiece_shape::fill::flat_over(&made, &self.id)?
        } else {
            None
        };
        let sheet = if let Some(flat) = flat {
            flat
        } else if made.len() == 4 && free {
            let four: [_; 4] = std::array::from_fn(|k| (made[k].curve.clone(), made[k].range));
            let mut sheet = workpiece_shape::between(&four, &self.id)?;
            sheet.faces[0].name = Some(format!("{}/face", self.id));
            sheet
        } else {
            ensure!(
                made.len() >= 2,
                "a hole of one side is filled only where it is flat"
            );
            filled_over(&made, &self.id)?
        };
        // Sewn to the surfaces whose free edges it runs along: one
        // surface with them (named as the first of them), a solid if
        // that closes it all around.
        let mut around: Vec<BodyId> = Vec::new();
        for side in sides {
            let Source::Topology(name) = &side.edge.source else {
                continue;
            };
            let body = topo.referred(name).and_then(|id| topo.owner(id));
            if let Some(body) = body
                && topo.body(body).is_some_and(|b| b.kind == BodyKind::Sheet)
                && !around.contains(&body)
            {
                around.push(body);
            }
        }
        let mut out = workpiece_shape::bodies::Out::default();
        if around.is_empty() {
            out.write_sheet(&sheet, &self.id);
            return Ok(out.made);
        }
        let mut parts = Vec::new();
        for &body in &around {
            parts.push(read_sheet(topo, body)?);
        }
        parts.push(sheet);
        let size = parts
            .iter()
            .map(|p| {
                let (lo, hi) = p.extent();
                (hi - lo).length()
            })
            .fold(0.0, f64::max);
        let (sewn, closed) = workpiece_boolean::stitched(&parts, size * 1e-6)
            .context("sewing the surface to those around it")?;
        let name = topo
            .name_of(around[0])
            .context("a surface without a name")?
            .to_owned();
        for &body in &around {
            out.remove(topo, body);
        }
        if closed {
            out.write(&sewn, &name);
        } else {
            out.write_sheet(&sewn, &name);
        }
        Ok(out.made)
    }

    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        (): &(),
    ) -> EditVisualization<Edit<BoundaryArgs, ()>> {
        let fields = workpiece_core::Arguments::fields(&self.args, context);
        let mut handles = BTreeMap::new();
        for k in 0..self.args.sides.len() {
            if let Some(handle) = leaving_handle(&context.world.topo, &self.args, k) {
                handles.insert(format!("side{k}"), handle);
            }
        }
        EditVisualization::new(VBox::new(fields), handles).map(Edit::args)
    }

    fn summary(&self) -> String {
        let n = self.args.sides.len();
        let held = self
            .args
            .sides
            .iter()
            .filter(|s| s.way != Continuity::Free)
            .count();
        match held {
            0 => format!("{n} sides"),
            _ => format!("{n} sides, {held} smooth"),
        }
    }
}

/// The box of the sides, and below it each side with how the surface
/// runs on from it.
fn sides_field(args: &BoundaryArgs, _: &EditContext<'_>) -> Vec<Widget<BoundaryArgs>> {
    let picked: Vec<Source> = args.sides.iter().map(|s| s.edge.source.clone()).collect();
    let was = args.clone();
    let set = move |picked: Vec<Source>| {
        // (Those kept keep how they run on.)
        let mut new = was.clone();
        new.sides = picked
            .into_iter()
            .map(|source| {
                was.sides
                    .iter()
                    .find(|s| s.edge.source == source)
                    .cloned()
                    .unwrap_or_else(|| BoundarySide::free(source.into()))
            })
            .collect();
        new
    };
    let mut out: Vec<Widget<BoundaryArgs>> = vec![
        ReferenceList::new("Sides", vec![ReferenceKind::Edge], picked, set)
            .required()
            .into(),
    ];
    if args.sides.is_empty() {
        out.push(
            Label::new(
                "",
                "Pick the edges around the hole, end to end: any number of them",
                Mood::Plain,
            )
            .into(),
        );
    }
    use workpiece_core::arguments::Choices;
    for (k, side) in args.sides.iter().enumerate() {
        let (now, all) = (args.clone(), Continuity::ALL);
        let name = side.edge.name().rsplit('/').take(2).collect::<Vec<_>>();
        let name = name.into_iter().rev().collect::<Vec<_>>().join("/");
        let choice = Choice::new(
            name,
            all.iter().map(|c| c.label().to_owned()).collect(),
            all.iter().position(|c| *c == side.way).unwrap_or(0),
            move |i| {
                let mut new = now.clone();
                new.sides[k].way = all[i.min(all.len() - 1)];
                new
            },
        )
        .with_icons(all.iter().map(|c| c.icon()).collect())
        .with_icon("f-edge")
        .drop_down();
        out.push(choice.into());
        if side.way != Continuity::Free {
            let (set, clear) = (args.clone(), args.clone());
            let face = ReferenceInput::of_kind(
                "From face",
                ReferenceKind::Face,
                side.face.as_ref().map(|f| f.source.clone()),
                move |source: Source| {
                    let mut new = set.clone();
                    new.sides[k].face = Some(source.into());
                    new
                },
            )
            .optional("The face it bounds", move || {
                let mut new = clear.clone();
                new.sides[k].face = None;
                new
            });
            out.push(face.into());
            out.push(leaving_row(args, k));
        }
    }
    out
}

/// How side `k` leaves: as full as, and turned by, numbers (and its
/// handle in the view), and the button that flips it.
fn leaving_row(args: &BoundaryArgs, k: usize) -> Widget<BoundaryArgs> {
    let side = &args.sides[k];
    let set = |put: fn(&mut BoundarySide, f64)| {
        let now = args.clone();
        move |x: f64| {
            let mut new = now.clone();
            put(&mut new.sides[k], x);
            new
        }
    };
    let flipped = {
        let now = args.clone();
        move || {
            let mut new = now.clone();
            new.sides[k].flip = !new.sides[k].flip;
            new
        }
    };
    HBox::new(vec![
        NumericInput::new(
            "Tangent length ×",
            side.magnitude,
            set(|s, x| s.magnitude = x),
        )
        .into(),
        NumericInput::new("Angle °", side.angle, set(|s, x| s.angle = x)).into(),
        ToolButton::new("flip", "Flip", Hotkey::key('X'), flipped)
            .with_hint("Leave the other way: back over the face beside it")
            .into(),
    ])
    .into()
}

/// The handle of how side `k` leaves, at the middle of its curve: a knob
/// at the tip of the way it leaves, moved in the tangent plane of the
/// face beside it — turning it, and pulling it out or in.
fn leaving_handle(
    topo: &Topo,
    args: &BoundaryArgs,
    k: usize,
) -> Option<workpiece_core::handles::Handle<BoundaryArgs>> {
    let side = &args.sides[k];
    let (solid, face, edge) = beside(topo, side).ok()??;
    let (curve, (t0, t1)) = curve_of(topo, &side.edge.source).ok()?;
    let t = (t0 + t1) / 2.0;
    let at = curve.point_at(t);
    // (As the side is now, and as it would be square and unflipped: what
    // its angle is from.)
    let now = across_from(&solid, face, edge, t, side);
    let square = BoundarySide {
        flip: false,
        magnitude: 1.0,
        angle: 0.0,
        ..side.clone()
    };
    let square = across_from(&solid, face, edge, t, &square).away;
    let reach = length_of(&curve, (t0, t1)) / 4.0;
    let tip = at + now.away * reach;
    let normal = now.normal;
    let was = args.clone();
    let handle = PlanarHandle::new(tip, normal, move |p| {
        let v = p - at;
        let v = v - normal * v.dot(normal);
        let mut new = was.clone();
        let side = &mut new.sides[k];
        side.magnitude = (v.length() / reach).max(1e-3);
        side.angle = libm::atan2(normal.cross(square).dot(v), square.dot(v)).to_degrees();
        side.flip = false;
        new
    })?
    .from(at);
    Some(handle.into())
}

/// How long `curve` is over `range`, about.
fn length_of(curve: &Curve3, (t0, t1): (f64, f64)) -> f64 {
    (0..32)
        .map(|k| {
            let at = |i: i32| curve.point_at(t0 + (t1 - t0) * f64::from(i) / 32.0);
            at(k).distance(at(k + 1))
        })
        .sum()
}
