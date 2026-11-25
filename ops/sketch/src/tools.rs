//! The sketch's tools: what a hover, a click and a drag on the canvas do.
//!
//! Drawing tools work by clicks: each click is snapped (to points, onto
//! curves, level with the click before) and, when the tool has enough of
//! them, becomes geometry with the constraints the snaps imply. The select
//! tool drags: points, curves and dimension texts follow the pointer as
//! far as the constraints allow, and a drag over nothing selects what is
//! inside it.

use workpiece_core::Edit;
use workpiece_core::handles::{CanvasEvent, CanvasEventKind, CanvasView};
use workpiece_core::math::Point2;
use workpiece_core::world::topo::nurbs::recognize::circumcenter;

use crate::geometry::{nearest_on_segment, sweep};
use crate::model::{Id, Item, Kind, Shape, SketchArgs, SketchAxis};
use crate::solve::Var;
use crate::{draw, modify};

/// How near, in pixels, the pointer has to be to point at something.
pub(crate) const REACH_PX: f64 = 8.0;

/// A tool of the sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tool {
    /// Select and drag.
    #[default]
    Select,
    /// A chain of lines and tangent arcs.
    Line,
    /// A rectangle by two corners.
    Rectangle,
    /// A rectangle by its center and a corner.
    CenterRectangle,
    /// A rectangle turned any way: two corners along one side, then how
    /// far the side across reaches.
    RotatedRectangle,
    /// A circle by its center and a point on it.
    Circle,
    /// A circle by the two ends of a diameter.
    Circle2,
    /// A circle through three points.
    Circle3,
    /// An ellipse by its center, the end of one axis, and how far out
    /// the other reaches.
    Ellipse,
    /// An arc by its ends and a point on it.
    Arc,
    /// An arc by its center and its ends.
    CenterArc,
    /// A slot by the centers of its ends and its width.
    Slot,
    /// A regular polygon by its center and a corner.
    Polygon,
    /// A spline by its control points.
    Spline,
    /// A point.
    Point,
    /// Text in a box: a turned rectangle by three clicks, as
    /// [`Tool::RotatedRectangle`] draws one, the letters inside it.
    Text,
    /// A rotation axis, through two clicks: the axis the sketch is
    /// turned about, a centre line without end.
    RotationAxis,
    /// A dimension of what is clicked.
    Dimension,
    /// Cut away the pieces of curves the pointer is dragged across.
    Trim,
}

/// A clicked position, and what it snapped to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    /// The position, after snapping.
    pub at: Point2,
    /// The point or entity it snapped to.
    pub to: Option<Item>,
    /// The second entity, where it snapped to the crossing of two.
    pub also: Option<Id>,
    /// Whether it snapped to the middle of the line `to`.
    pub middle: bool,
    /// The axis it is level with the click before along.
    pub along: Option<SketchAxis>,
    /// A point hovered before that it is level with, and the axis along
    /// which.
    pub aligned: Option<(Id, SketchAxis)>,
    /// Another point hovered before that it is level with as well, along
    /// the other axis: where the two cross.
    pub aligned_too: Option<(Id, SketchAxis)>,
    /// Whether it carries the chain on straight from the arc it ends
    /// with: the line to it is tangent to the arc.
    pub tangent: bool,
}

impl Snap {
    /// `at`, snapped to nothing.
    pub fn free(at: Point2) -> Self {
        Self {
            at,
            to: None,
            also: None,
            middle: false,
            along: None,
            aligned: None,
            aligned_too: None,
            tangent: false,
        }
    }

    /// `at`, snapped to `to`.
    fn to(at: Point2, to: Item) -> Self {
        Self {
            to: Some(to),
            ..Self::free(at)
        }
    }
}

/// A chain of lines and arcs being drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chain {
    /// The point the chain started at: clicking it closes the chain.
    pub first: Id,
    /// The point the next segment starts at.
    pub last: Id,
    /// The segment that ends there.
    pub previous: Id,
    /// The direction the chain arrives at `last` with.
    pub tangent: Point2,
    /// Whether the next segment is a tangent arc instead of a line.
    pub arc: bool,
    /// Whether the pointer has left `last` since the last switch between
    /// line and arc: hovering back over it switches.
    pub armed: bool,
}

/// What the pointer holds while its button is down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grab {
    /// What was under it when the button went down.
    pub item: Option<Item>,
    /// Where the button went down.
    pub start: Point2,
    /// Where the pointer was last.
    pub at: Point2,
    /// Whether it holds a rotation axis by its grip: dragging turns it.
    pub turning: bool,
}

/// The state of a sketch's edit session.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchState {
    /// The tool in use.
    pub tool: Tool,
    /// The clicks of the shape being drawn.
    pub clicks: Vec<Snap>,
    /// The chain being drawn with the line tool.
    pub chain: Option<Chain>,
    /// Where the pointer is, snapped, while a drawing tool is in use.
    pub cursor: Option<Snap>,
    /// What is under the pointer.
    pub hover: Option<Item>,
    /// What is selected, in the order selected.
    pub selection: Vec<Item>,
    /// What the pointer holds.
    pub grab: Option<Grab>,
    /// The corners of the selection rectangle being dragged.
    pub band: Option<(Point2, Point2)>,
    /// The path of a trim being dragged.
    pub trail: Vec<Point2>,
    /// How the plane was last seen: decides what horizontal means.
    pub view: CanvasView,
    /// How many sides the polygon tool makes.
    pub sides: usize,
    /// How many there are of what a pattern repeats, the original
    /// counted.
    pub copies: usize,
    /// Whether the spline tool's clicks are points the curve goes
    /// through (else they are its control points).
    pub through: bool,
    /// The angle Rotate turns by, in degrees.
    pub angle: f64,
    /// What Scale multiplies sizes by.
    pub factor: f64,
    /// What Text writes (its capitals half as high as the box drawn
    /// for it, to change in its panel).
    pub text: String,
    /// The dimension just placed, or double-clicked: the field of its
    /// value takes the keyboard, so its value is typed next.
    pub focus: Option<Id>,
    /// Whether what the drawing tools make is construction geometry.
    pub construction: bool,
    /// Whether there is a grid: shown, and what a drawing tool clicks
    /// free of anything else lands on.
    pub grid: bool,
    /// The degree of the splines the spline tool makes.
    pub degree: usize,
    /// The points a drawing tool hovered last, latest first: the next
    /// point snaps level with them.
    pub remembered: Vec<Id>,
    /// What the user was last told, until the next click.
    pub notice: Option<String>,
}

impl Default for SketchState {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            clicks: Vec::new(),
            chain: None,
            cursor: None,
            hover: None,
            selection: Vec::new(),
            grab: None,
            band: None,
            trail: Vec::new(),
            view: CanvasView::default(),
            sides: 6,
            copies: 3,
            through: false,
            angle: 90.0,
            factor: 2.0,
            text: "Text".into(),
            focus: None,
            construction: false,
            grid: false,
            degree: 3,
            remembered: Vec::new(),
            notice: None,
        }
    }
}

impl SketchState {
    /// The sketch axis that shows as horizontal in the view.
    pub fn horizontal(&self) -> SketchAxis {
        SketchAxis::nearest(self.view.right)
    }

    /// The same state, with nothing half-drawn.
    #[must_use]
    pub fn idle(mut self) -> Self {
        self.clicks.clear();
        self.chain = None;
        self.cursor = None;
        self.grab = None;
        self.band = None;
        self.trail.clear();
        self.focus = None;
        self.remembered.clear();
        self.notice = None;
        self
    }
}

type SketchEdit = Edit<SketchArgs, SketchState>;

/// The rotation axis whose grip is under `at`, if one is.
pub(crate) fn grip(args: &SketchArgs, at: Point2, view: &CanvasView) -> Option<Id> {
    let reach = REACH_PX * view.pixel;
    args.entities
        .keys()
        .filter(|&&id| args.is_rotation_axis(id))
        .find(|&&id| draw::axis_grip(args, id, view).is_some_and(|(g, _)| g.distance(at) <= reach))
        .copied()
}

/// The point or entity of `args` under `at`: points first, then
/// dimension texts and badges, then curves (a rotation axis by its grip
/// too).
pub(crate) fn hit(args: &SketchArgs, at: Point2, view: &CanvasView) -> Option<Item> {
    let reach = REACH_PX * view.pixel;
    let nearest = |candidates: Vec<(Item, f64)>| {
        candidates
            .into_iter()
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(item, _)| item)
    };
    // (Not the control points along a curve off the model.)
    let of_model = args.projected_points();
    let points = args
        .points
        .iter()
        .filter(|(id, _)| !args.is_axis_end(**id) && of_model.get(*id) != Some(&false))
        .map(|(&id, p)| (Item::Point(id), p.at.distance(at)))
        .collect();
    let labels = args
        .constraints
        .keys()
        .map(|&id| {
            let d = draw::label_position(args, id, view).distance(at);
            // Texts are wider than they are high.
            (Item::Constraint(id), d / 1.6)
        })
        .collect();
    let curves = |axes: bool| {
        args.entities
            .keys()
            .filter(|&&id| args.is_axis(id) == axes)
            .map(|&id| (Item::Entity(id), args.distance_to(id, at)))
            .collect()
    };
    nearest(points)
        .or_else(|| grip(args, at, view).map(Item::Entity))
        .or_else(|| nearest(labels))
        .or_else(|| nearest(curves(false)))
        // The axes, where nothing else is.
        .or_else(|| nearest(curves(true)))
}

/// `at` snapped, to the first of: a point; where two curves cross; the
/// middle of a line; onto a line or circle; level with `level_with` or
/// with one of the points `remembered` (horizontally or vertically, as
/// the view shows it); onto an axis of the plane.
pub(crate) fn snap(
    args: &SketchArgs,
    at: Point2,
    view: &CanvasView,
    level_with: Option<Point2>,
    state: &SketchState,
) -> Snap {
    let remembered = &state.remembered[..];
    let reach = REACH_PX * view.pixel;
    let point = args
        .points
        .iter()
        .filter(|(id, _)| !args.is_axis_end(**id))
        .map(|(&id, p)| (id, p.at, p.at.distance(at)))
        .filter(|c| c.2 <= reach)
        .min_by(|a, b| a.2.total_cmp(&b.2));
    if let Some((id, p, _)) = point {
        return Snap::to(p, Item::Point(id));
    }
    // Where two curves cross.
    let near: Vec<(Id, crate::trim::Carrier)> = args
        .entities
        .keys()
        .filter(|&&id| args.distance_to(id, at) <= 2.0 * reach)
        .filter_map(|&id| Some((id, crate::trim::carrier(args, id)?)))
        .collect();
    let mut crossing: Option<(f64, Point2, Id, Id)> = None;
    for (i, (a, ca)) in near.iter().enumerate() {
        for (b, cb) in &near[i + 1..] {
            for p in crate::trim::crossings(ca, cb, crate::trim::touch_gap(args)) {
                let d = p.distance(at);
                if d <= reach && crossing.is_none_or(|c| d < c.0) {
                    crossing = Some((d, p, *a, *b));
                }
            }
        }
    }
    if let Some((_, p, a, b)) = crossing {
        return Snap {
            also: Some(b),
            ..Snap::to(p, Item::Entity(a))
        };
    }
    // The middle of a line.
    let middle = args
        .entities
        .keys()
        .filter(|&&id| !args.is_endless(id))
        .filter_map(|&id| {
            let (a, b) = args.line(id)?;
            Some((id, (a + b) / 2.0))
        })
        .filter(|(_, m)| m.distance(at) <= reach)
        .min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)));
    if let Some((id, m)) = middle {
        return Snap {
            middle: true,
            ..Snap::to(m, Item::Entity(id))
        };
    }
    // Onto a curve; onto an axis only after everything else.
    let onto = |axes: bool| {
        for &id in args.entities.keys() {
            if args.is_axis(id) != axes || args.distance_to(id, at) > reach {
                continue;
            }
            let on = if let Some((a, b)) = args.line(id) {
                if args.is_endless(id) {
                    // Onto the axis, wherever along it.
                    let along = (b - a).normalize_or_zero();
                    Some(a + along * (at - a).dot(along))
                } else {
                    Some(nearest_on_segment(at, a, b))
                }
            } else {
                args.circle(id)
                    .and_then(|(_, c, r)| Some(c + (at - c).try_normalize()? * r))
            };
            if let Some(on) = on {
                return Some(Snap::to(on, Item::Entity(id)));
            }
        }
        None
    };
    if let Some(snap) = onto(false) {
        return snap;
    }
    // Level with the click before, and with a point hovered before: each
    // along one axis, so both can hold at once (a corner).
    let mut out = Snap::free(at);
    let horizontal = SketchAxis::nearest(view.right);
    let level = |p: Point2, from: Point2, axis: SketchAxis| {
        let d = p - from;
        let off = d.dot(axis.other().unit());
        (off.abs() <= reach && d.dot(axis.unit()).abs() > reach)
            .then(|| p - axis.other().unit() * off)
    };
    if let Some(from) = level_with {
        for axis in [horizontal, horizontal.other()] {
            if let Some(p) = level(out.at, from, axis) {
                out.at = p;
                out.along = Some(axis);
                break;
            }
        }
    }
    // (With one point along one axis and with another along the other:
    // where the two cross.)
    for &id in remembered {
        let Some(from) = args.points.get(&id).map(|p| p.at) else {
            continue;
        };
        if level_with.is_some_and(|l| l.distance(from) <= reach) {
            continue;
        }
        for axis in [horizontal, horizontal.other()] {
            let taken = out.along == Some(axis)
                || out.aligned.is_some_and(|a| a.1 == axis)
                || out.aligned_too.is_some();
            if taken || (out.along.is_some() && out.aligned.is_some()) {
                continue;
            }
            if let Some(p) = level(out.at, from, axis) {
                out.at = p;
                if out.aligned.is_none() {
                    out.aligned = Some((id, axis));
                } else {
                    out.aligned_too = Some((id, axis));
                }
                break;
            }
        }
    }
    if out.along.is_some() || out.aligned.is_some() {
        // Level with one thing only, it is still free along that level:
        // with a grid, it lands on the grid's lines across it.
        let held: Vec<SketchAxis> = [
            out.along,
            out.aligned.map(|a| a.1),
            out.aligned_too.map(|a| a.1),
        ]
        .into_iter()
        .flatten()
        .collect();
        if let (true, &[axis]) = (state.grid, &held[..]) {
            let step = grid_step(view);
            let along = out.at.dot(axis.unit());
            out.at += axis.unit() * ((along / step).round() * step - along);
        }
        return out;
    }
    // Or onto an axis; or, with a grid, onto the grid.
    onto(true).unwrap_or_else(|| {
        if state.grid {
            let step = grid_step(view);
            out.at = (out.at / step).round() * step;
        }
        out
    })
}

/// Lines, circles and points drawn elsewhere — the views of a drawing
/// sheet — snapped to as the sketcher snaps to its own: to a point,
/// where two cross, the middle of a line, onto a line or a circle, or
/// level with the point before.
#[derive(Debug, Clone, Default)]
pub struct Snappable(SketchArgs);

impl Snappable {
    /// The `lines`, the circles (middle and radius) `rounds`, and the
    /// `points` besides their ends and middles, to snap to.
    pub fn new(lines: &[(Point2, Point2)], rounds: &[(Point2, f64)], points: &[Point2]) -> Self {
        let mut args = SketchArgs::default();
        for (a, b) in lines {
            let (a, b) = (args.add_point(*a), args.add_point(*b));
            args.add_line(a, b);
        }
        for (centre, radius) in rounds {
            let centre = args.add_point(*centre);
            args.add(Shape::Circle {
                center: centre,
                radius: *radius,
            });
        }
        for p in points {
            args.add_point(*p);
        }
        Self(args)
    }

    /// `at` snapped, seen as `view` has it (how large a pixel is, which
    /// way is right), level with `level_with` where it nearly is: where
    /// it lands, and whether that is on something drawn.
    pub fn snap(
        &self,
        at: Point2,
        view: &CanvasView,
        level_with: Option<Point2>,
    ) -> (Point2, bool) {
        let snapped = snap(&self.0, at, view, level_with, &SketchState::default());
        (snapped.at, snapped.to.is_some())
    }
}

/// How far apart the lines of the grid are at this zoom: 1, 2 or 5 of a
/// power of ten, the least that leaves them some pixels apart.
pub(crate) fn grid_step(view: &CanvasView) -> f64 {
    const APART_PX: f64 = 14.0;
    let least = APART_PX * view.pixel;
    let ten = 10f64.powf(least.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|k| k * ten)
        .find(|step| *step >= least)
        .unwrap_or(10.0 * ten)
}

/// [`snap`] for the next point of the line tool's chain: level with the
/// chain's end, and, after an arc, straight on from it.
fn chain_snap(args: &SketchArgs, state: &SketchState, at: Point2, view: &CanvasView) -> Snap {
    let from = state
        .chain
        .map(|c| args.at(c.last))
        .or(state.clicks.last().map(|s| s.at));
    let found = snap(args, at, view, from, state);
    let after_arc = state
        .chain
        .filter(|c| !c.arc && args.circle(c.previous).is_some());
    if let (Some(chain), None, Some(from)) = (after_arc, found.to, from) {
        let reach = 2.0 * REACH_PX * view.pixel;
        let d = at - from;
        let (along, off) = (d.dot(chain.tangent), d.dot(chain.tangent.perp()));
        if along > reach && off.abs() <= reach {
            return Snap {
                tangent: true,
                ..Snap::free(from + chain.tangent * along)
            };
        }
    }
    found
}

/// [`snap`] for the clicks of a rotation axis or of a turned rectangle's
/// first side: the second level with the first.
fn axis_snap(args: &SketchArgs, state: &SketchState, at: Point2, view: &CanvasView) -> Snap {
    let from = match &state.clicks[..] {
        [first] => Some(first.at),
        _ => None,
    };
    snap(args, at, view, from, state)
}

/// [`snap`] for the next control point of the spline being drawn: onto
/// its first one too, which closes it.
fn spline_snap(args: &SketchArgs, state: &SketchState, at: Point2, view: &CanvasView) -> Snap {
    match state.clicks.first() {
        Some(first)
            if state.clicks.len() >= 2 && first.at.distance(at) <= REACH_PX * view.pixel =>
        {
            Snap::free(first.at)
        }
        _ => snap(args, at, view, None, state),
    }
}

/// Whether the clicks of a spline end where they began.
fn closes(clicks: &[Snap], pixel: f64) -> bool {
    match (clicks.first(), clicks.last()) {
        (Some(first), Some(last)) => {
            clicks.len() >= 3 && first.at.distance(last.at) <= REACH_PX * pixel * 1e-6
        }
        _ => false,
    }
}

/// Ties `point` to what `snap` snapped to.
fn attach(args: &mut SketchArgs, point: Id, snap: &Snap) {
    match snap.to {
        Some(Item::Point(q)) if q != point => {
            args.constrain(Kind::Coincident(point, q));
        }
        Some(Item::Entity(line)) if snap.middle => {
            args.constrain(Kind::Midpoint { point, line });
        }
        Some(Item::Entity(entity))
            if args.line(entity).is_some() || args.circle(entity).is_some() =>
        {
            args.constrain(Kind::PointOn { point, entity });
        }
        _ => {}
    }
    for (other, axis) in [snap.aligned, snap.aligned_too].into_iter().flatten() {
        if other != point && args.points.contains_key(&other) {
            args.constrain(Kind::Along {
                a: other,
                b: point,
                axis,
            });
        }
    }
    if let Some(entity) = snap.also {
        args.constrain(Kind::PointOn { point, entity });
    }
}

/// A new point at `snap`, tied to what it snapped to.
fn point_at(args: &mut SketchArgs, snap: &Snap) -> Id {
    let id = args.add_point(snap.at);
    attach(args, id, snap);
    id
}

/// The center and the far end's tangent of the arc that leaves `from`
/// along `tangent` and ends at `to`; `None` when that is a straight line.
pub(crate) fn tangent_arc(from: Point2, tangent: Point2, to: Point2) -> Option<(Point2, bool)> {
    let normal = tangent.perp();
    let chord = to - from;
    let across = chord.dot(normal);
    if across.abs() <= 1e-9 * chord.length() || chord.length_squared() == 0.0 {
        return None;
    }
    let radius = chord.length_squared() / (2.0 * across);
    // Counter-clockwise when the center is to the left.
    Some((from + normal * radius, radius > 0.0))
}

/// Adds a rectangle with corners `a` and `c` along the sketch's axes;
/// returns its corner points and its lines.
fn rectangle(args: &mut SketchArgs, a: Point2, c: Point2) -> ([Id; 4], [Id; 4]) {
    let corners = [a, Point2::new(c.x, a.y), c, Point2::new(a.x, c.y)];
    let p = corners.map(|at| args.add_point(at));
    let mut lines = [0; 4];
    for k in 0..4 {
        let (from, to) = (p[k], p[(k + 1) % 4]);
        lines[k] = args.add_line(from, to);
        let axis = if k % 2 == 0 {
            SketchAxis::U
        } else {
            SketchAxis::V
        };
        args.constrain(Kind::Along {
            a: from,
            b: to,
            axis,
        });
    }
    (p, lines)
}

/// Adds a rectangle from `a` to `b` along one side and `height` across
/// it (to the left of the way from `a` to `b`, or the right where
/// negative): its corners (`a`, `b`, then on around) and its sides, held
/// square — each side at right angles to the one before, the opposite
/// ones parallel.
fn turned_rectangle(
    args: &mut SketchArgs,
    a: Point2,
    b: Point2,
    height: f64,
) -> ([Id; 4], [Id; 4]) {
    let across = (b - a).normalize_or_zero().perp() * height;
    let corners = [a, b, b + across, a + across];
    let p = corners.map(|at| args.add_point(at));
    let lines = [0, 1, 2, 3].map(|k| args.add_line(p[k], p[(k + 1) % 4]));
    args.constrain(Kind::Perpendicular(lines[0], lines[1]));
    args.constrain(Kind::Parallel(lines[0], lines[2]));
    args.constrain(Kind::Parallel(lines[1], lines[3]));
    (p, lines)
}

/// How high the capitals of a text in a box `height` high are, at
/// first: half the box, to two figures.
pub(crate) fn text_size(height: f64) -> f64 {
    let half = height / 2.0;
    if half <= 0.0 || !half.is_finite() {
        return 1.0;
    }
    let ten = 10f64.powf(half.log10().floor() - 1.0);
    (half / ten).round() * ten
}

/// How far across the line from `a` to `b` the click `at` is: to its
/// left positive.
fn across(a: Point2, b: Point2, at: Point2) -> f64 {
    (b - a).normalize_or_zero().perp_dot(at - a)
}

/// Makes the shape the tool has all its clicks for. Returns what it made,
/// for selecting; `None` if the clicks do not make a shape.
fn build(args: &mut SketchArgs, state: &SketchState) -> Option<Vec<Item>> {
    let clicks = &state.clicks;
    let apart = |a: usize, b: usize| clicks[a].at.distance(clicks[b].at) > 1e-12;
    let made = match (state.tool, clicks.len()) {
        (Tool::RotatedRectangle | Tool::Text, 3) if apart(0, 1) => {
            let (a, b) = (clicks[0].at, clicks[1].at);
            let height = across(a, b, clicks[2].at);
            if height.abs() <= 1e-12 {
                return None;
            }
            // (Text reads from the first click to the second, upright:
            // its box goes up from the side clicked.)
            let (p, lines) = turned_rectangle(args, a, b, height);
            attach(args, p[0], &clicks[0]);
            attach(args, p[1], &clicks[1]);
            // Level as drawn, it stays level.
            if let Some(axis) = clicks[1].along {
                args.constrain(Kind::Along {
                    a: p[0],
                    b: p[1],
                    axis,
                });
            }
            if state.tool == Tool::RotatedRectangle {
                lines.to_vec()
            } else {
                for line in lines {
                    args.entities.get_mut(&line)?.construction = true;
                }
                // The box is laid out from its lower left: with the
                // height to the right of the first side, that is the
                // far corner of it.
                let corners = if height > 0.0 {
                    p
                } else {
                    [p[3], p[2], p[1], p[0]]
                };
                vec![args.add(Shape::Text(crate::model::TextBox {
                    corners,
                    text: state.text.clone(),
                    size: text_size(height.abs()),
                    align: crate::model::Align::Start,
                    valign: crate::model::Align::Start,
                    wrap: true,
                }))]
            }
        }
        (Tool::Point, 1) => {
            let p = point_at(args, &clicks[0]);
            vec![args.add(Shape::Point(p))]
        }
        (Tool::Rectangle, 2) if apart(0, 1) => {
            let d = clicks[1].at - clicks[0].at;
            if d.x == 0.0 || d.y == 0.0 {
                return None;
            }
            let (p, lines) = rectangle(args, clicks[0].at, clicks[1].at);
            attach(args, p[0], &clicks[0]);
            attach(args, p[2], &clicks[1]);
            lines.to_vec()
        }
        (Tool::CenterRectangle, 2) if apart(0, 1) => {
            let (m, c) = (clicks[0].at, clicks[1].at);
            if (c - m).x == 0.0 || (c - m).y == 0.0 {
                return None;
            }
            let (p, lines) = rectangle(args, m * 2.0 - c, c);
            attach(args, p[2], &clicks[1]);
            // The center is the middle of a diagonal.
            let diagonal = args.add_line(p[0], p[2]);
            let center = point_at(args, &clicks[0]);
            let dot = args.add(Shape::Point(center));
            for id in [diagonal, dot] {
                args.entities.get_mut(&id)?.construction = true;
            }
            args.constrain(Kind::Midpoint {
                point: center,
                line: diagonal,
            });
            lines.to_vec()
        }
        (Tool::Circle, 2) if apart(0, 1) => {
            let center = point_at(args, &clicks[0]);
            let radius = clicks[0].at.distance(clicks[1].at);
            vec![args.add(Shape::Circle { center, radius })]
        }
        (Tool::Circle2, 2) if apart(0, 1) => {
            let (a, b) = (clicks[0].at, clicks[1].at);
            let center = args.add_point((a + b) / 2.0);
            let radius = a.distance(b) / 2.0;
            vec![args.add(Shape::Circle { center, radius })]
        }
        (Tool::Circle3, 3) => {
            let (a, b, on) = (clicks[0].at, clicks[1].at, clicks[2].at);
            let c = circumcenter(a, b, on)?;
            let center = args.add_point(c);
            let radius = c.distance(a);
            vec![args.add(Shape::Circle { center, radius })]
        }
        (Tool::Ellipse, 3) if apart(0, 1) => {
            let c = clicks[0].at;
            let (hull, weights, knots) = ellipse(c, clicks[1].at, clicks[2].at)?;
            // Its centre, to dimension and constrain from.
            let center = point_at(args, &clicks[0]);
            let dot = args.add(Shape::Point(center));
            args.entities.get_mut(&dot)?.construction = true;
            // A closed curve of four quarters: it ends on the point it
            // starts on.
            let mut control: Vec<Id> = hull[..8].iter().map(|at| args.add_point(*at)).collect();
            control.push(control[0]);
            vec![args.add(Shape::Spline {
                control,
                degree: 2,
                knots,
                weights,
            })]
        }
        (Tool::Arc, 3) => {
            let (a, b, on) = (clicks[0].at, clicks[1].at, clicks[2].at);
            let c = circumcenter(a, b, on)?;
            // Counter-clockwise from the start, the arc passes the third
            // click before its end.
            let forward = sweep(a - c, on - c) < sweep(a - c, b - c);
            let (s, e) = if forward { (0, 1) } else { (1, 0) };
            let center = args.add_point(c);
            let (start, end) = (point_at(args, &clicks[s]), point_at(args, &clicks[e]));
            vec![args.add(Shape::Arc { center, start, end })]
        }
        (Tool::CenterArc, 3) if apart(0, 1) && apart(0, 2) => {
            let (c, a) = (clicks[0].at, clicks[1].at);
            let b = c + (clicks[2].at - c).normalize() * c.distance(a);
            let forward = (a - c).perp_dot(b - c) >= 0.0;
            let center = point_at(args, &clicks[0]);
            let first = point_at(args, &clicks[1]);
            let second = args.add_point(b);
            let (start, end) = if forward {
                (first, second)
            } else {
                (second, first)
            };
            vec![args.add(Shape::Arc { center, start, end })]
        }
        (Tool::Slot, 3) if apart(0, 1) => {
            let (c1, c2) = (clicks[0].at, clicks[1].at);
            let axis = (c2 - c1).normalize();
            let n = axis.perp();
            let r = (clicks[2].at - c1).dot(n).abs();
            if r <= 1e-12 {
                return None;
            }
            let (m1, m2) = (point_at(args, &clicks[0]), point_at(args, &clicks[1]));
            let p = [c1 + n * r, c1 - n * r, c2 - n * r, c2 + n * r].map(|at| args.add_point(at));
            let arc1 = args.add(Shape::Arc {
                center: m1,
                start: p[0],
                end: p[1],
            });
            let arc2 = args.add(Shape::Arc {
                center: m2,
                start: p[2],
                end: p[3],
            });
            let (l1, l2) = (args.add_line(p[1], p[2]), args.add_line(p[3], p[0]));
            for (line, arc) in [(l1, arc1), (l1, arc2), (l2, arc1), (l2, arc2)] {
                args.constrain(Kind::Tangent {
                    a: line,
                    b: arc,
                    internal: false,
                });
            }
            args.constrain(Kind::Equal(arc1, arc2));
            vec![arc1, l1, arc2, l2]
        }
        (Tool::RotationAxis, 2) if apart(0, 1) => {
            let through = point_at(args, &clicks[0]);
            let axis = args.add_rotation_axis(through, clicks[1].at - clicks[0].at);
            // Through what the second click snapped to; level with the
            // first, along the plane's axis.
            match clicks[1].to {
                Some(Item::Point(point)) if point != through => {
                    args.constrain(Kind::PointOn {
                        point,
                        entity: axis,
                    });
                }
                _ => {}
            }
            if let Some(level) = clicks[1].along.and_then(|a| args.plane_axis(a)) {
                args.constrain(Kind::Parallel(axis, level));
            }
            vec![axis]
        }
        (Tool::Polygon, 2) if apart(0, 1) => {
            let (c, v) = (clicks[0].at, clicks[1].at);
            let n = state.sides.clamp(3, 64);
            let center = point_at(args, &clicks[0]);
            let circle = args.add(Shape::Circle {
                center,
                radius: c.distance(v),
            });
            args.entities.get_mut(&circle)?.construction = true;
            let step = std::f64::consts::TAU / n as f64;
            let corners: Vec<Id> = (0..n)
                .map(|k| args.add_point(c + Point2::from_angle(step * k as f64).rotate(v - c)))
                .collect();
            attach(args, corners[0], &clicks[1]);
            let mut lines = Vec::new();
            for k in 0..n {
                lines.push(args.add_line(corners[k], corners[(k + 1) % n]));
                args.constrain(Kind::PointOn {
                    point: corners[k],
                    entity: circle,
                });
            }
            for &line in &lines[1..] {
                args.constrain(Kind::Equal(lines[0], line));
            }
            lines
        }
        _ => return None,
    };
    Some(made.into_iter().map(Item::Entity).collect())
}

/// The ellipse about `centre` with one axis out to `end` and the other
/// as far out as `beside` is from that axis: the nine control points
/// of its four quarters (the last the first again), their weights, and
/// the knots.
pub(crate) fn ellipse(
    centre: Point2,
    end: Point2,
    beside: Point2,
) -> Option<(Vec<Point2>, Vec<f64>, Vec<f64>)> {
    let a = end - centre;
    let across = a.try_normalize()?.perp();
    let reach = (beside - centre).dot(across).abs();
    if reach <= 1e-12 {
        return None;
    }
    let b = across * reach;
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let hull = [a, a + b, b, b - a, -a, -a - b, -b, a - b, a]
        .iter()
        .map(|d| centre + *d)
        .collect();
    let weights = vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0];
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0];
    Some((hull, weights, knots))
}

/// How many clicks a tool's shape takes; `None` for tools that take any
/// number, or none.
fn clicks_needed(tool: Tool) -> Option<usize> {
    match tool {
        Tool::Point => Some(1),
        Tool::Rectangle
        | Tool::CenterRectangle
        | Tool::Circle
        | Tool::Circle2
        | Tool::Polygon
        | Tool::RotationAxis => Some(2),
        Tool::Arc
        | Tool::CenterArc
        | Tool::Slot
        | Tool::Circle3
        | Tool::Ellipse
        | Tool::RotatedRectangle
        | Tool::Text => Some(3),
        _ => None,
    }
}

/// Whether the tool ties its click number `index` (from 0) to what the
/// click snapped to; the other clicks only take the snapped position.
pub(crate) fn attaches(tool: Tool, index: usize) -> bool {
    match tool {
        Tool::Line | Tool::Spline | Tool::Point => true,
        Tool::RotationAxis => index == 0,
        Tool::Circle | Tool::Ellipse => index == 0,
        Tool::Circle2 | Tool::Circle3 => false,
        Tool::Arc => index < 2,
        Tool::Rectangle
        | Tool::CenterRectangle
        | Tool::RotatedRectangle
        | Tool::Text
        | Tool::CenterArc
        | Tool::Slot
        | Tool::Polygon => index < 2,
        Tool::Select | Tool::Dimension | Tool::Trim => false,
    }
}

/// Ends what is half-drawn: a spline with the control points clicked so
/// far is made, anything else is dropped.
pub(crate) fn finish(args: &SketchArgs, state: &SketchState) -> SketchEdit {
    let mut args = args.clone();
    let mut done = state.clone().idle();
    if state.tool == Tool::Spline {
        let mut clicks = state.clicks.clone();
        clicks.dedup_by(|b, a| a.at.distance(b.at) <= REACH_PX * state.view.pixel);
        if clicks.len() >= 2 {
            // (Through the clicks, if that is asked: the control points
            // of the curve that does. Its ends are the clicks' own.)
            let places: Vec<Point2> = clicks.iter().map(|s| s.at).collect();
            let hull = if state.through {
                crate::geometry::interpolated(&places, state.degree).unwrap_or(places)
            } else {
                places
            };
            let control: Vec<Id> = hull.iter().map(|at| args.add_point(*at)).collect();
            for (&p, s) in [
                (control.first(), clicks.first()),
                (control.last(), clicks.last()),
            ]
            .into_iter()
            .filter_map(|(p, s)| p.zip(s))
            {
                attach(&mut args, p, s);
            }
            // One that comes back to where it began ends there: a loop.
            if let (Some(&first), Some(&last)) = (control.first(), control.last())
                && closes(&clicks, state.view.pixel)
            {
                args.constrain(Kind::Coincident(last, first));
            }
            let spline = args.add(Shape::Spline {
                control,
                degree: state.degree,
                knots: Vec::new(),
                weights: Vec::new(),
            });
            if let Some(entity) = args.entities.get_mut(&spline) {
                entity.construction = state.construction;
            }
            args.solve();
            done.selection = vec![Item::Entity(spline)];
            return Edit::both(args, done);
        }
    }
    Edit::state(done)
}

/// Switches to `tool`, ending what is half-drawn.
pub(crate) fn switch(args: &SketchArgs, state: &SketchState, tool: Tool) -> SketchEdit {
    let mut edit = finish(args, state);
    if let Some(s) = &mut edit.state {
        if tool == Tool::Select && state.tool == Tool::Select {
            s.selection.clear();
        }
        // The dimension tool starts from what is selected; drawing tools
        // from nothing.
        if !matches!(tool, Tool::Select | Tool::Dimension) {
            s.selection.clear();
        }
        s.tool = tool;
    }
    edit
}

/// The next segment of the line tool's chain: a line, or a tangent arc,
/// from the chain's end (or the first click) to `to`.
fn chain_segment(args: &mut SketchArgs, state: &SketchState, to: &Snap) -> Option<SketchState> {
    let mut next = state.clone();
    let reach = REACH_PX * state.view.pixel;
    let (start, from) = match (&state.chain, state.clicks.first()) {
        (Some(chain), _) => (chain.last, args.at(chain.last)),
        (None, Some(first)) => (point_at(args, first), first.at),
        (None, None) => {
            next.clicks = vec![*to];
            return Some(next);
        }
    };
    if from.distance(to.at) <= reach {
        return None;
    }
    // Clicking the chain's first point closes it.
    let closes = state.chain.filter(|c| {
        to.to == Some(Item::Point(c.first)) || args.at(c.first).distance(to.at) <= reach
    });
    let end = match closes {
        Some(chain) => chain.first,
        None => point_at(args, to),
    };
    let arc = state
        .chain
        .filter(|c| c.arc)
        .and_then(|c| Some((c, tangent_arc(from, c.tangent, to.at)?)));
    let (segment, tangent) = if let Some((chain, (center, forward))) = arc {
        let center_at = center;
        let center = args.add_point(center);
        let (s, e) = if forward { (start, end) } else { (end, start) };
        let segment = args.add(Shape::Arc {
            center,
            start: s,
            end: e,
        });
        // Tangent to what came before; two circles touch inside when one
        // center lies within the other circle.
        let internal = args
            .circle(chain.previous)
            .is_some_and(|(_, c, r)| c.distance(center_at) < r.max(center_at.distance(to.at)));
        args.constrain(Kind::Tangent {
            a: chain.previous,
            b: segment,
            internal,
        });
        // Level ends, if the arc was drawn so.
        if let (Some(axis), None) = (to.along, closes) {
            args.constrain(Kind::Along {
                a: start,
                b: end,
                axis,
            });
        }
        let radial = to.at - center_at;
        let leaving = if forward {
            radial.perp()
        } else {
            -radial.perp()
        };
        (segment, leaving.normalize())
    } else {
        let segment = args.add_line(start, end);
        if let (Some(axis), None) = (to.along, closes) {
            args.constrain(Kind::Along {
                a: start,
                b: end,
                axis,
            });
        }
        // Straight on from an arc: tangent to it.
        if let (true, Some(chain)) = (to.tangent, state.chain) {
            args.constrain(Kind::Tangent {
                a: segment,
                b: chain.previous,
                internal: false,
            });
        }
        (segment, (to.at - from).normalize())
    };
    args.solve();
    next.clicks.clear();
    next.selection.clear();
    next.chain = closes.is_none().then(|| Chain {
        first: state.chain.map_or(start, |c| c.first),
        last: end,
        previous: segment,
        tangent,
        // After an arc, lines again.
        arc: false,
        armed: false,
    });
    Some(next)
}

/// What `event` on the canvas does to the sketch `args` in `state`.
pub(crate) fn handle(
    args: &SketchArgs,
    state: &SketchState,
    event: CanvasEvent,
) -> Option<SketchEdit> {
    let mut next = state.clone();
    next.view = event.view;
    let view = &event.view;
    let at = event.at;
    let drawing = !matches!(state.tool, Tool::Select | Tool::Dimension | Tool::Trim);
    let mut edited: Option<SketchArgs> = None;
    // A click anywhere gives the keyboard back, and ends what was said.
    if matches!(
        event.kind,
        CanvasEventKind::Press | CanvasEventKind::Click | CanvasEventKind::DoubleClick
    ) {
        next.focus = None;
        next.notice = None;
    }

    match (event.kind, state.tool) {
        (CanvasEventKind::Hover, _) => {
            if drawing {
                let cursor = match state.tool {
                    Tool::Line => chain_snap(args, state, at, view),
                    Tool::Spline => spline_snap(args, state, at, view),
                    Tool::RotationAxis | Tool::RotatedRectangle | Tool::Text => {
                        axis_snap(args, state, at, view)
                    }
                    _ => snap(args, at, view, None, state),
                };
                // What it would snap to lights up; a point is remembered,
                // for snapping level with it.
                next.hover = cursor.to;
                if let Some(Item::Point(id)) = cursor.to {
                    next.remembered.retain(|p| *p != id);
                    next.remembered.insert(0, id);
                    next.remembered.truncate(2);
                }
                next.remembered.retain(|p| args.points.contains_key(p));
                next.cursor = Some(cursor);
                // Back over the chain's end: switch between line and arc.
                if let Some(chain) = &mut next.chain {
                    let over = args.at(chain.last).distance(at) <= REACH_PX * view.pixel;
                    if over && chain.armed {
                        chain.arc = !chain.arc;
                        chain.armed = false;
                    } else if !over {
                        chain.armed = true;
                    }
                }
            } else {
                next.hover = hit(args, at, view);
                // The dimension tool shows the dimension where the pointer
                // would put it.
                next.cursor = (state.tool == Tool::Dimension).then_some(Snap::free(at));
            }
        }
        (CanvasEventKind::Click, Tool::Select) => {
            let hit = hit(args, at, view);
            match (hit, event.add) {
                (Some(item), true) => match next.selection.iter().position(|s| *s == item) {
                    Some(i) => drop(next.selection.remove(i)),
                    None => next.selection.push(item),
                },
                (Some(item), false) => next.selection = vec![item],
                (None, true) => {}
                (None, false) => next.selection.clear(),
            }
        }
        (CanvasEventKind::Click, Tool::Trim) => {
            edited = crate::trim::trim(args, &[at], view);
        }
        (CanvasEventKind::Click, Tool::Dimension) => match hit(args, at, view) {
            Some(item) => {
                // Starting over after a dimension was placed.
                if matches!(next.selection[..], [Item::Constraint(_)]) {
                    next.selection.clear();
                }
                if !next.selection.contains(&item) {
                    next.selection.push(item);
                }
            }
            None => {
                let (kind, value, diameter) =
                    modify::dimension_for(args, &state.selection, at, view)?;
                let (new, id, notice) = modify::place_dimension(args, kind, value, at, diameter);
                next.selection = vec![Item::Constraint(id)];
                // Its value is typed next, in its panel (a driven one
                // has none to type: it says so).
                next.focus = notice.is_none().then_some(id);
                next.notice = notice;
                edited = Some(new);
            }
        },
        (CanvasEventKind::Click, Tool::Line) => {
            let to = chain_snap(args, state, at, view);
            let mut new = args.clone();
            next = chain_segment(&mut new, &next, &to)?;
            if new != *args {
                edited = Some(new);
            }
        }
        (CanvasEventKind::Click, Tool::Spline) => {
            next.clicks.push(spline_snap(args, state, at, view));
            // Back at its beginning, it is done.
            if closes(&next.clicks, view.pixel) {
                return Some(finish(args, &next));
            }
        }
        (CanvasEventKind::Click, tool) => {
            next.clicks.push(match tool {
                Tool::RotationAxis | Tool::RotatedRectangle | Tool::Text => {
                    axis_snap(args, state, at, view)
                }
                _ => snap(args, at, view, None, state),
            });
            if Some(next.clicks.len()) == clicks_needed(tool) {
                let mut new = args.clone();
                let made = build(&mut new, &next);
                next.clicks.clear();
                if let Some(made) = made {
                    new.solve();
                    next.selection = made;
                    edited = Some(new);
                }
            }
        }
        (CanvasEventKind::DoubleClick, Tool::Spline | Tool::Line) => {
            return Some(finish(args, state));
        }
        (CanvasEventKind::DoubleClick, Tool::Select | Tool::Dimension) => {
            // A dimension's value is typed in its panel.
            if let Some(Item::Constraint(id)) = hit(args, at, view)
                && args
                    .constraints
                    .get(&id)
                    .is_some_and(|c| c.dimension.is_some())
            {
                next.selection = vec![Item::Constraint(id)];
                if args.is_driven(id) {
                    next.notice = Some(
                        "A driven dimension only measures: make it driving (in its panel) to give it a value"
                            .to_owned(),
                    );
                } else {
                    next.focus = Some(id);
                }
            }
        }
        (CanvasEventKind::Press, Tool::Select | Tool::Trim) => {
            let select = state.tool == Tool::Select;
            next.grab = Some(Grab {
                item: hit(args, at, view).filter(|_| select),
                start: at,
                at,
                turning: select && grip(args, at, view).is_some(),
            });
            next.trail = vec![at];
        }
        (CanvasEventKind::Drag, Tool::Trim) => next.trail.push(at),
        (CanvasEventKind::Release, Tool::Trim) => {
            edited = crate::trim::trim(args, &state.trail, view);
            next.trail.clear();
            next.grab = None;
        }
        (CanvasEventKind::Drag, Tool::Select) => {
            let grab = next.grab.as_mut()?;
            let delta = at - grab.at;
            grab.at = at;
            let mut new = args.clone();
            match grab.item {
                None => next.band = Some((grab.start, at)),
                Some(Item::Constraint(id)) => {
                    if let Some(d) = new
                        .constraints
                        .get_mut(&id)
                        .and_then(|c| c.dimension.as_mut())
                    {
                        d.label += delta;
                    }
                }
                Some(Item::Entity(e)) if grab.turning => {
                    // Turned by its grip: towards the pointer.
                    let from = new.line(e).map_or(at, |l| l.0);
                    if let Some(towards) = (at - from).try_normalize() {
                        new.pull(&[(Var::Du(e), towards.x), (Var::Dv(e), towards.y)]);
                    }
                }
                Some(Item::Point(p)) => drop(new.pull(&[(Var::X(p), at.x), (Var::Y(p), at.y)])),
                Some(Item::Entity(e)) => {
                    let pulls: Vec<(Var, f64)> = match new.entities.get(&e).map(|e| &e.shape) {
                        Some(Shape::Circle { center, .. }) => {
                            vec![(Var::R(e), new.at(*center).distance(at))]
                        }
                        _ => new
                            .points_of(e)
                            .into_iter()
                            .flat_map(|p| {
                                let to = new.at(p) + delta;
                                [(Var::X(p), to.x), (Var::Y(p), to.y)]
                            })
                            .collect(),
                    };
                    new.pull(&pulls);
                }
            }
            if new != *args {
                edited = Some(new);
            }
        }
        (CanvasEventKind::Release, Tool::Select) => {
            if let Some((a, b)) = next.band.take() {
                let (lo, hi) = (a.min(b), a.max(b));
                let inside = |p: Point2| p.cmpge(lo).all() && p.cmple(hi).all();
                let within = |&id: &Id| {
                    let points = args.points_of(id);
                    let round = match args.entities[&id].shape {
                        Shape::Circle { center, radius } => {
                            let c = args.at(center);
                            inside(c - Point2::splat(radius)) && inside(c + Point2::splat(radius))
                        }
                        _ => true,
                    };
                    round && points.iter().all(|&p| inside(args.at(p)))
                };
                let picked = args
                    .entities
                    .keys()
                    .filter(|id| !args.is_endless(**id) && within(id))
                    .map(|&id| Item::Entity(id));
                if !event.add {
                    next.selection.clear();
                }
                for item in picked {
                    if !next.selection.contains(&item) {
                        next.selection.push(item);
                    }
                }
            }
            next.grab = None;
        }
        _ => {}
    }

    // In construction mode, what a drawing tool made is construction
    // geometry.
    if let (true, true, Some(new)) = (drawing, state.construction, &mut edited) {
        for (id, entity) in &mut new.entities {
            if !args.entities.contains_key(id) {
                entity.construction = true;
            }
        }
    }

    match (edited, next != *state) {
        (Some(args), true) => Some(Edit::both(args, next)),
        (Some(args), false) => Some(Edit::args(args)),
        (None, true) => Some(Edit::state(next)),
        (None, false) => None,
    }
}
