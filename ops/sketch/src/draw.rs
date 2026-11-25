//! What a sketch looks like while it is edited: its curves colored by
//! whether they can still move, a badge for every constraint, and
//! dimensions drawn as on a drawing (thin extension and dimension lines,
//! slender filled arrowheads, the value written along the line above it).

use shapecraft_core::handles::{CanvasShape, CanvasView, Tone};
use shapecraft_core::math::Point2;

use crate::geometry::{angle, unit};
use crate::model::{Constraint, DimTolerance, Id, Item, Kind, Shape, SketchArgs, SketchAxis};
use crate::solve::Diagnosis;
use crate::tools::{self, SketchState, Tool};

/// Sizes, in pixels.
const CURVE: f32 = 1.8;
const CONSTRUCTION: f32 = 1.2;
const THIN: f32 = crate::layout::THIN;
const POINT: f32 = 6.0;
/// How far badges sit from what they belong to, and from each other.
const BADGE_LIFT: f64 = 14.0;
const BADGE_STEP: f64 = 20.0;

/// `value` as a drawing shows it: up to three decimals, no trailing zeros.
pub(crate) fn number(value: f64) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// A deviation with its sign: `+0.2`, `−0.1`, `0`.
fn deviation(value: f64) -> String {
    match number(value.abs()).as_str() {
        "0" => "0".to_owned(),
        text if value > 0.0 => format!("+{text}"),
        text => format!("−{text}"),
    }
}

/// The text of a dimension: its value with what kind it is (`R`, `Ø`,
/// `°`) and its tolerance; then the upper and lower deviations, which are
/// set small after it.
pub(crate) fn dimension_text(constraint: &Constraint) -> (String, String, String) {
    let Some(d) = &constraint.dimension else {
        return Default::default();
    };
    let value = number(d.value);
    let prefix = match constraint.kind {
        _ if d.diameter => "Ø",
        Kind::Size(_) => "R",
        _ => "",
    };
    let mut text = match constraint.kind {
        Kind::Angle(..) => format!("{value}°"),
        _ => format!("{prefix}{value}"),
    };
    let (mut upper, mut lower) = (String::new(), String::new());
    // A driven dimension only measures: in brackets, without a
    // tolerance.
    if d.driven {
        return (format!("({text})"), upper, lower);
    }
    match &d.tolerance {
        DimTolerance::General => {}
        DimTolerance::Reference => text = format!("({text})"),
        DimTolerance::Symmetric(t) => text += &format!(" ±{}", number(t.abs())),
        DimTolerance::Deviations { upper: u, lower: l } => {
            upper = deviation(*u);
            lower = deviation(*l);
        }
        // The limits of size, the upper over the lower, in place of
        // the value.
        DimTolerance::Limits { upper: u, lower: l } => {
            let unit = match constraint.kind {
                Kind::Angle(..) => "°",
                _ => "",
            };
            text = prefix.to_owned();
            upper = format!("{}{unit}", number(d.value + u));
            lower = format!("{}{unit}", number(d.value + l));
        }
        DimTolerance::Fit(class) => text += &format!(" {class}"),
    }
    (text, upper, lower)
}

/// The letters on a constraint's badge.
fn badge(kind: &Kind, horizontal: SketchAxis) -> &'static str {
    match kind {
        Kind::Coincident(..) => "◎",
        Kind::PointOn { .. } => "on",
        Kind::Along { axis, .. } if *axis == horizontal => "H",
        Kind::Along { .. } => "V",
        Kind::Parallel(..) => "//",
        Kind::Perpendicular(..) => "⟂",
        Kind::Collinear(..) => "≡",
        Kind::Tangent { .. } => "T",
        Kind::Smooth { .. } => "G2",
        Kind::Equal(..) => "=",
        Kind::Midpoint { .. } => "M",
        Kind::Symmetric { .. } => "S",
        _ => "",
    }
}

/// What a constraint's badge belongs to: its first entity, else its first
/// point.
fn owner(kind: &Kind) -> Option<Item> {
    let (points, entities) = kind.operands();
    match kind {
        // On the line they level, if they are a line's ends.
        Kind::PointOn { point, .. } | Kind::Midpoint { point, .. } => Some(Item::Point(*point)),
        _ => entities
            .first()
            .map(|&e| Item::Entity(e))
            .or(points.first().map(|&p| Item::Point(p))),
    }
}

/// Where the text of constraint `id` is: a dimension's label, a badge
/// beside what it belongs to (badges of the same owner in a row).
pub(crate) fn label_position(args: &SketchArgs, id: Id, view: &CanvasView) -> Point2 {
    let Some(constraint) = args.constraints.get(&id) else {
        return Point2::ZERO;
    };
    if let Some(d) = &constraint.dimension {
        return d.label;
    }
    let mut owner = owner(&constraint.kind);
    // A level line's badge sits on the line, not on its end.
    if let Kind::Along { a, b, .. } = constraint.kind {
        let line = args.entities.iter().find(
            |(_, e)| matches!(e.shape, Shape::Line(p, q) if (p, q) == (a, b) || (p, q) == (b, a)),
        );
        owner = line.map(|(&e, _)| Item::Entity(e)).or(owner);
    }
    let base = match owner {
        Some(Item::Entity(e)) => args.middle(e),
        Some(Item::Point(p)) => args.at(p),
        _ => Point2::ZERO,
    };
    let row = args
        .constraints
        .range(..id)
        .filter(|(_, c)| c.dimension.is_none() && same_owner(args, c, owner))
        .count();
    base + (view.up * BADGE_LIFT + view.right * (BADGE_LIFT + BADGE_STEP * row as f64)) * view.pixel
}

fn same_owner(args: &SketchArgs, c: &Constraint, of: Option<Item>) -> bool {
    let mut owner = owner(&c.kind);
    if let Kind::Along { a, b, .. } = c.kind {
        let line = args.entities.iter().find(
            |(_, e)| matches!(e.shape, Shape::Line(p, q) if (p, q) == (a, b) || (p, q) == (b, a)),
        );
        owner = line.map(|(&e, _)| Item::Entity(e)).or(owner);
    }
    owner == of
}

/// How far a rotation axis reaches either way of the middle of the
/// view, in pixels: past the corners of any screen.
const AXIS_REACH: f64 = 3000.0;
/// How far from the point it passes through a rotation axis's grip is,
/// in pixels.
const GRIP: f64 = 36.0;

/// The ends of what is drawn of rotation axis `id`: across the whole of
/// the view, about its middle.
pub(crate) fn axis_span(args: &SketchArgs, id: Id, view: &CanvasView) -> (Point2, Point2) {
    let Some((a, b)) = args.line(id) else {
        return (Point2::ZERO, Point2::ZERO);
    };
    let along = (b - a).normalize_or_zero();
    let foot = a + along * (view.middle - a).dot(along);
    let reach = along * AXIS_REACH * view.pixel;
    (foot - reach, foot + reach)
}

/// Where the grip of rotation axis `id` is, that turns it, and the way
/// it points: along the axis, a little off the point it passes through.
pub(crate) fn axis_grip(args: &SketchArgs, id: Id, view: &CanvasView) -> Option<(Point2, Point2)> {
    let (a, b) = args.line(id)?;
    let along = (b - a).try_normalize()?;
    Some((a + along * GRIP * view.pixel, along))
}

/// Collects shapes.
struct Pen<'a> {
    out: Vec<CanvasShape>,
    view: &'a CanvasView,
}

impl Pen<'_> {
    fn px(&self, pixels: f64) -> f64 {
        pixels * self.view.pixel
    }

    fn line(&mut self, points: Vec<Point2>, tone: Tone, width: f32, dashed: bool) {
        if points.len() >= 2 {
            self.out.push(CanvasShape::Polyline {
                points,
                tone,
                width,
                dashed,
            });
        }
    }

    /// A centre line: a long dash, a gap, a dot, a gap, along `points`.
    fn chain_dotted(&mut self, points: Vec<Point2>, tone: Tone, width: f32) {
        let pattern =
            [(14.0, true), (4.0, false), (2.0, true), (4.0, false)].map(|(l, on)| (self.px(l), on));
        let (mut k, mut left) = (0, pattern[0].0);
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let (total, mut at) = (a.distance(b), 0.0);
            let along = (b - a).try_normalize().unwrap_or(Point2::X);
            while at < total {
                let step = left.min(total - at);
                if pattern[k].1 {
                    self.line(
                        vec![a + along * at, a + along * (at + step)],
                        tone,
                        width,
                        false,
                    );
                }
                at += step;
                left -= step;
                if left <= 1e-12 {
                    k = (k + 1) % pattern.len();
                    left = pattern[k].0;
                }
            }
        }
    }

    fn arrow(&mut self, tip: Point2, direction: Point2, tone: Tone) {
        self.out.push(CanvasShape::Arrowhead {
            tip,
            direction,
            tone,
        });
    }

    /// The dimension `c` of the sketch, as the shared layout draws it.
    fn dimension(&mut self, args: &SketchArgs, c: &Constraint, tone: Tone) {
        self.out
            .extend(crate::layout::dimension(args, c, self.view, tone));
    }
}

/// The shapes that show the sketch `args` in `state`, seen in `view`.
pub(crate) fn shapes(
    args: &SketchArgs,
    state: &SketchState,
    diagnosis: &Diagnosis,
    view: &CanvasView,
) -> Vec<CanvasShape> {
    let mut pen = Pen {
        out: Vec::new(),
        view,
    };
    let marked = |item: Item, otherwise: Tone| {
        if state.selection.contains(&item) {
            Tone::Selected
        } else if state.hover == Some(item) {
            Tone::Hover
        } else {
            otherwise
        }
    };

    // The grid, under everything: around what is drawn and where the
    // pointer is, every fifth line a little stronger.
    if state.grid {
        let step = crate::tools::grid_step(view);
        let (mut lo, mut hi) = (Point2::INFINITY, Point2::NEG_INFINITY);
        let shown = args
            .points
            .values()
            .map(|p| p.at)
            .chain(state.cursor.map(|c| c.at))
            .chain([Point2::ZERO]);
        for p in shown {
            (lo, hi) = (lo.min(p), hi.max(p));
        }
        // (No more lines than can be told apart.)
        const MOST: f64 = 240.0;
        let pad = (pen.px(600.0)).min(step * MOST / 4.0);
        let from = ((lo - Point2::splat(pad)) / step).floor();
        let to = ((hi + Point2::splat(pad)) / step).ceil();
        let to = to.min(from + Point2::splat(MOST));
        for axis in 0..2 {
            let (first, last) = (from[axis] as i64, to[axis] as i64);
            for k in first..=last {
                let at = k as f64 * step;
                let (a, b) = if axis == 0 {
                    (Point2::new(at, from.y * step), Point2::new(at, to.y * step))
                } else {
                    (Point2::new(from.x * step, at), Point2::new(to.x * step, at))
                };
                let width = if k % 5 == 0 { 0.5 } else { 0.25 };
                pen.line(vec![a, b], Tone::Construction, width, false);
            }
        }
    }

    // The plane's axes through its origin, as far as anything is drawn.
    let reach = 2.0 * args.scale().max(pen.px(400.0));
    for (&id, entity) in &args.entities {
        if let (Some(_), Some((a, b))) = (entity.axis, args.line(id)) {
            let along = (b - a).normalize_or_zero() * reach;
            let tone = marked(Item::Entity(id), Tone::Construction);
            let width = if tone == Tone::Construction {
                0.6
            } else {
                CONSTRUCTION
            };
            pen.line(vec![a - along, a + along], tone, width, false);
        }
    }
    // A rotation axis: a centre line across the whole view, and a grip
    // that turns it.
    for (&id, entity) in &args.entities {
        if let Shape::Axis { .. } = entity.shape {
            let (from, to) = axis_span(args, id, view);
            let base = if diagnosis.free_entities.contains(&id) {
                Tone::Free
            } else {
                Tone::Construction
            };
            pen.chain_dotted(vec![from, to], marked(Item::Entity(id), base), CURVE);
            if let Some((at, direction)) = axis_grip(args, id, view) {
                pen.arrow(at, direction, marked(Item::Entity(id), Tone::Badge));
            }
        }
    }

    let offered: std::collections::BTreeSet<Id> = args
        .entities
        .iter()
        .filter(|(id, e)| e.auto && !crate::project::used(args, **id))
        .map(|(&id, _)| id)
        .collect();
    let offered_points: std::collections::BTreeSet<Id> =
        offered.iter().flat_map(|&id| args.points_of(id)).collect();
    for (&id, entity) in &args.entities {
        if args.is_endless(id) {
            continue;
        }
        let free = args
            .points_of(id)
            .iter()
            .any(|p| diagnosis.free_points.contains(p))
            || diagnosis.free_entities.contains(&id);
        let base = if entity.projected.is_some() {
            Tone::Projected
        } else if entity.construction {
            Tone::Construction
        } else if free {
            Tone::Free
        } else {
            Tone::Constrained
        };
        let tone = marked(Item::Entity(id), base);
        // What is offered unasked is not drawn until it is pointed at
        // or selected (or something is built on it).
        if offered.contains(&id) && tone == base {
            continue;
        }
        let width = if entity.construction {
            CONSTRUCTION
        } else {
            CURVE
        };
        pen.line(args.polyline(id), tone, width, entity.construction);
        // A spline's hull — of one that is the sketch's own: a curve
        // off the model is just its line.
        if let Shape::Spline { control, .. } = &entity.shape
            && entity.projected.is_none()
        {
            let hull = control.iter().map(|&p| args.at(p)).collect();
            pen.line(hull, Tone::Construction, 0.8, true);
        }
    }
    let of_model = args.projected_points();
    for (&id, point) in &args.points {
        if args.is_axis_end(id) {
            continue;
        }
        let base = if point.fixed || !diagnosis.free_points.contains(&id) {
            Tone::Constrained
        } else {
            Tone::Free
        };
        let tone = marked(Item::Point(id), base);
        // A projected curve is just its line: its points show only
        // while they are pointed at or selected.
        if (of_model.contains_key(&id) || offered_points.contains(&id)) && tone == base {
            continue;
        }
        let size = if tone == Tone::Hover {
            POINT + 3.0
        } else {
            POINT
        };
        pen.out.push(CanvasShape::Dot {
            at: point.at,
            tone,
            size,
        });
    }

    // Ends that join nothing, with another such end near: a gap in what
    // looks like an outline. A ring round each.
    let open = crate::profile::open_ends(args);
    for (k, &at) in open.iter().enumerate() {
        let near = open
            .iter()
            .enumerate()
            .any(|(j, &o)| j != k && o.distance(at) <= 40.0 * view.pixel);
        if near {
            let ring = (0..=24)
                .map(|n| {
                    at + unit(std::f64::consts::TAU * f64::from(n) / 24.0) * (6.0 * view.pixel)
                })
                .collect();
            pen.line(ring, Tone::Conflict, 1.5, false);
        }
    }

    let horizontal = state.horizontal();
    for (&id, constraint) in &args.constraints {
        let base = if diagnosis.conflicting.contains(&id) || diagnosis.redundant.contains(&id) {
            Tone::Conflict
        } else if constraint.dimension.is_some() {
            Tone::Dimension
        } else {
            Tone::Badge
        };
        let tone = marked(Item::Constraint(id), base);
        if !args.shows(id) {
            continue;
        }
        if constraint.dimension.is_some() {
            pen.dimension(args, constraint, tone);
        } else {
            pen.out.push(CanvasShape::Text {
                at: label_position(args, id, view),
                along: view.right,
                text: badge(&constraint.kind, horizontal).to_owned(),
                upper: String::new(),
                lower: String::new(),
                tone,
                badge: true,
            });
        }
    }

    preview(&mut pen, args, state);
    pen.out
}

/// What the tool in use is about to make.
fn preview(pen: &mut Pen<'_>, args: &SketchArgs, state: &SketchState) {
    let tone = Tone::Preview;
    if let Some((a, b)) = state.band {
        let corners = vec![a, Point2::new(b.x, a.y), b, Point2::new(a.x, b.y), a];
        pen.line(corners, Tone::Selected, THIN, true);
    }
    if state.trail.len() >= 2 {
        pen.line(state.trail.clone(), Tone::Conflict, CURVE, false);
    }
    let Some(cursor) = state.cursor else {
        return;
    };
    let to = cursor.at;
    if state.tool == Tool::Dimension {
        // The dimension of what is picked, where the pointer would put
        // it; not over something else to pick, nor once it is placed.
        let placed = matches!(state.selection[..], [Item::Constraint(_)]);
        if state.hover.is_none() && !placed {
            let fits = crate::modify::dimension_for(args, &state.selection, to, pen.view);
            if let Some((kind, value, diameter)) = fits {
                let mut draft = args.clone();
                let id = draft.dimension_of(kind, value, to, diameter);
                pen.dimension(&draft, &draft.constraints[&id], tone);
            }
        }
        return;
    }
    let clicks: Vec<Point2> = state.clicks.iter().map(|s| s.at).collect();
    let circle = |c: Point2, r: f64| -> Vec<Point2> {
        (0..=64)
            .map(|k| c + unit(std::f64::consts::TAU * f64::from(k) / 64.0) * r)
            .collect()
    };
    let arc = |c: Point2, from: Point2, to: Point2| -> Vec<Point2> {
        let total = crate::geometry::sweep(from - c, to - c);
        let (a0, r) = (angle(from - c), c.distance(from));
        (0..=48)
            .map(|k| c + unit(a0 + total * f64::from(k) / 48.0) * r)
            .collect()
    };
    let rectangle =
        |a: Point2, c: Point2| vec![a, Point2::new(c.x, a.y), c, Point2::new(a.x, c.y), a];
    match (state.tool, &clicks[..]) {
        (Tool::Line, _) => {
            let from = state
                .chain
                .map(|c| args.at(c.last))
                .or(clicks.first().copied());
            let bend = state
                .chain
                .filter(|c| c.arc)
                .zip(from)
                .and_then(|(c, from)| tools::tangent_arc(from, c.tangent, to));
            match (bend, from) {
                (Some((center, true)), Some(from)) => {
                    pen.line(arc(center, from, to), tone, CURVE, false);
                }
                (Some((center, false)), Some(from)) => {
                    pen.line(arc(center, to, from), tone, CURVE, false);
                }
                (_, Some(from)) => pen.line(vec![from, to], tone, CURVE, false),
                (_, None) => {}
            }
            if let (Some(from), Some(_)) = (from, cursor.along) {
                let text = if cursor.along == Some(state.horizontal()) {
                    "H"
                } else {
                    "V"
                };
                pen.out.push(CanvasShape::Text {
                    at: (from + to) / 2.0 + pen.view.up * pen.px(BADGE_LIFT),
                    along: pen.view.right,
                    text: text.to_owned(),
                    upper: String::new(),
                    lower: String::new(),
                    tone,
                    badge: true,
                });
            }
        }
        (Tool::Rectangle, [a]) => pen.line(rectangle(*a, to), tone, CURVE, false),
        (Tool::CenterRectangle, [m]) => pen.line(rectangle(*m * 2.0 - to, to), tone, CURVE, false),
        (Tool::Circle, [c]) => pen.line(circle(*c, c.distance(to)), tone, CURVE, false),
        (Tool::Circle2, [a]) => {
            pen.line(
                circle((*a + to) / 2.0, a.distance(to) / 2.0),
                tone,
                CURVE,
                false,
            );
        }
        (Tool::Circle3, [a]) => pen.line(vec![*a, to], tone, THIN, true),
        (Tool::Circle3, [a, b]) => {
            use shapecraft_core::world::topo::nurbs::recognize::circumcenter;
            if let Some(c) = circumcenter(*a, *b, to) {
                pen.line(circle(c, c.distance(*a)), tone, CURVE, false);
            }
        }
        (Tool::Ellipse, [c]) => pen.line(vec![*c, to], tone, THIN, true),
        (Tool::Ellipse, [c, a]) => {
            let across = (*a - *c).perp().normalize_or_zero();
            let reach = (to - *c).dot(across);
            let around = (0..=64)
                .map(|k| {
                    let (sin, cos) = (std::f64::consts::TAU * f64::from(k) / 64.0).sin_cos();
                    *c + (*a - *c) * cos + across * (reach * sin)
                })
                .collect();
            pen.line(around, tone, CURVE, false);
            pen.line(vec![*c, *a], tone, THIN, true);
        }
        (Tool::Polygon, [c]) => {
            let n = state.sides.clamp(3, 64);
            let step = std::f64::consts::TAU / n as f64;
            let corners = (0..=n)
                .map(|k| *c + Point2::from_angle(step * k as f64).rotate(to - *c))
                .collect();
            pen.line(corners, tone, CURVE, false);
            pen.line(circle(*c, c.distance(to)), tone, THIN, true);
        }
        (Tool::Arc, [a]) => pen.line(vec![*a, to], tone, THIN, true),
        (Tool::Arc, [a, b]) => {
            use shapecraft_core::world::topo::nurbs::recognize::circumcenter;
            if let Some(c) = circumcenter(*a, *b, to) {
                let forward =
                    crate::geometry::sweep(*a - c, to - c) < crate::geometry::sweep(*a - c, *b - c);
                let (s, e) = if forward { (*a, *b) } else { (*b, *a) };
                pen.line(arc(c, s, e), tone, CURVE, false);
            }
        }
        (Tool::CenterArc, [c]) => {
            pen.line(vec![*c, to], tone, THIN, true);
            pen.line(circle(*c, c.distance(to)), tone, THIN, true);
        }
        (Tool::CenterArc, [c, a]) => {
            if let Some(dir) = (to - *c).try_normalize() {
                let b = *c + dir * c.distance(*a);
                let (s, e) = if (*a - *c).perp_dot(b - *c) >= 0.0 {
                    (*a, b)
                } else {
                    (b, *a)
                };
                pen.line(arc(*c, s, e), tone, CURVE, false);
                pen.line(vec![*c, b], tone, THIN, true);
            }
        }
        (Tool::Slot, [a]) => pen.line(vec![*a, to], tone, THIN, true),
        (Tool::Slot, [c1, c2]) => {
            if let Some(axis) = (*c2 - *c1).try_normalize() {
                let n = axis.perp();
                let r = (to - *c1).dot(n).abs();
                let p = [*c1 + n * r, *c1 - n * r, *c2 - n * r, *c2 + n * r];
                let mut outline = arc(*c1, p[0], p[1]);
                outline.extend(arc(*c2, p[2], p[3]));
                outline.push(p[0]);
                pen.line(outline, tone, CURVE, false);
            }
        }
        (Tool::Spline, _) if !clicks.is_empty() => {
            let mut hull = clicks.clone();
            hull.push(to);
            pen.line(hull.clone(), tone, THIN, true);
            // The curve the hull makes so far.
            let mut draft = SketchArgs::default();
            let control = hull.iter().map(|&p| draft.add_point(p)).collect();
            let id = draft.add(Shape::Spline {
                control,
                degree: 3,
                knots: Vec::new(),
                weights: Vec::new(),
            });
            pen.line(draft.polyline(id), tone, CURVE, false);
        }
        _ => {}
    }
    let size = if cursor.to.is_some() {
        POINT + 4.0
    } else {
        POINT
    };
    pen.out.push(CanvasShape::Dot { at: to, tone, size });
    // The constraint a click here would make with what it snaps to.
    // Level with a point hovered before: a guide from it.
    for (id, axis) in [cursor.aligned, cursor.aligned_too].into_iter().flatten() {
        let from = args.at(id);
        pen.line(vec![from, to], tone, THIN, true);
        let text = if axis == state.horizontal() { "H" } else { "V" };
        pen.out.push(CanvasShape::Text {
            at: (from + to) / 2.0 + pen.view.up * pen.px(BADGE_LIFT),
            along: pen.view.right,
            text: text.to_owned(),
            upper: String::new(),
            lower: String::new(),
            tone,
            badge: true,
        });
    }
    let text = match cursor.to {
        _ if cursor.tangent => "T",
        Some(Item::Entity(_)) if cursor.middle => "M",
        Some(Item::Entity(_)) if cursor.also.is_some() => "×",
        Some(Item::Point(_)) => "◎",
        Some(Item::Entity(_)) => "on",
        _ => "",
    };
    if !text.is_empty() && tools::attaches(state.tool, state.clicks.len()) {
        let lift = (pen.view.up - pen.view.right) * pen.px(BADGE_LIFT);
        pen.out.push(CanvasShape::Text {
            at: to + lift,
            along: pen.view.right,
            text: text.to_owned(),
            upper: String::new(),
            lower: String::new(),
            tone,
            badge: true,
        });
    }
}
