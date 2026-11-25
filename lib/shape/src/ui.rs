//! What the dialogs of these operations share: fields for the sketch,
//! the bodies, and picking a sketch's areas in the view.

use std::rc::Rc;

use workpiece_core::handles::{Canvas, CanvasEventKind, CanvasShape, Handle, Tone};
use workpiece_core::math::Point2;
use workpiece_core::widgets::{ReferenceInput, ToolButton};
use workpiece_core::world::topo::BodyId;
use workpiece_core::world::{AreaPick, Profile};
use workpiece_core::{Hotkey, Reference, Sketched, Widget, World};

use crate::sweep;

/// A function that gives the arguments `args` with `x` put in by `put`.
pub fn set<A: Clone + 'static, X>(
    args: &A,
    put: impl Fn(&mut A, X) + 'static,
) -> impl Fn(X) -> A + Clone + 'static {
    let put = Rc::new(put);
    let args = args.clone();
    move |x| {
        let mut new = args.clone();
        put(&mut new, x);
        new
    }
}

/// A reference to a sketch.
pub type SketchRef = Reference<Sketched>;
/// A reference to a body.
pub type BodyRef = Reference<BodyId>;

/// The profile `sketch` names in `world`.
pub fn profile<'a>(world: &'a World, sketch: &Option<SketchRef>) -> Option<&'a Profile> {
    world.profiles.get(sketch.as_ref()?.name())
}

/// The field that picks the sketch.
pub fn sketch_field<A: 'static>(
    sketch: &Option<SketchRef>,
    with: impl Fn(Option<SketchRef>) -> A + 'static,
) -> Widget<A> {
    ReferenceInput::new("Sketch", sketch.clone(), move |s| with(Some(s))).into()
}

/// The small button that turns a sweep the other way, to sit beside its
/// distance or angle.
pub fn flip<A: Clone + 'static>(args: &A, flip: impl Fn(&mut A) + 'static) -> Widget<A> {
    let flipped = set(args, move |a, (): ()| flip(a));
    ToolButton::new("flip", "Flip", Hotkey::key('X'), move || flipped(()))
        .with_hint("Go the other way")
        .into()
}

/// The middle of what is swept of `profile` (the areas `picked`, or all
/// of it), in its plane: where the handle of a sweep starts.
pub fn middle(profile: &Profile, picked: &[AreaPick]) -> Point2 {
    let regions = sweep::regions(profile, picked).unwrap_or_default();
    let mut points: Vec<Point2> = regions
        .iter()
        .flat_map(|&r| sweep::outline_points(profile, &profile.regions[r].outer))
        .collect();
    if points.is_empty() {
        points = (0..profile.edges.len())
            .flat_map(|e| sweep::outline_points(profile, &[(e, false)]))
            .chain(profile.vertices.iter().map(|v| v.point))
            .collect();
    }
    let lo = points.iter().copied().reduce(Point2::min);
    let hi = points.iter().copied().reduce(Point2::max);
    lo.zip(hi).map_or(Point2::ZERO, |(lo, hi)| (lo + hi) / 2.0)
}

/// The sketch's plane as a handle that picks its areas: a click in an
/// area takes it, or lets it go. What will be swept shows highlighted.
pub fn area_picker<A: 'static>(
    profile: &Profile,
    picked: &[AreaPick],
    with: impl Fn(Vec<AreaPick>) -> A + 'static,
) -> Handle<A> {
    let profile = Rc::new(profile.clone());
    let picked: Rc<Vec<AreaPick>> = Rc::new(picked.to_vec());
    let swept = sweep::regions(&profile, &picked).unwrap_or_default();
    let anchor = profile.vertices.first().map_or(Point2::ZERO, |v| v.point);
    let plane = profile.plane;
    let draw = {
        let profile = profile.clone();
        move |_: &_| {
            let line = |points: Vec<Point2>, tone, width| CanvasShape::Polyline {
                points,
                tone,
                width,
                dashed: false,
            };
            let mut shapes: Vec<CanvasShape> = (0..profile.edges.len())
                .map(|e| {
                    let mut points = sweep::outline_points(&profile, &[(e, false)]);
                    let edge = &profile.edges[e];
                    points.push(edge.curve.point_at(edge.t1));
                    line(points, Tone::Free, 1.2)
                })
                .collect();
            for &region in &swept {
                let r = &profile.regions[region];
                for walk in std::iter::once(&r.outer).chain(&r.holes) {
                    let mut points = sweep::outline_points(&profile, walk);
                    points.extend(points.first().copied());
                    shapes.push(line(points, Tone::Selected, 2.6));
                }
            }
            shapes
        }
    };
    let on_event = move |event: workpiece_core::handles::CanvasEvent| {
        if event.kind != CanvasEventKind::Click {
            return None;
        }
        let region = sweep::region_at(&profile, event.at)?;
        let mut new: Vec<AreaPick> = (*picked).clone();
        let had = new
            .iter()
            .position(|pick| sweep::picked_region(&profile, pick).ok() == Some(region));
        // A click picks an area by its boundary: it says which side of
        // which curves, wherever the click was.
        match had {
            Some(i) => drop(new.remove(i)),
            None => new.push(AreaPick::Bounded(profile.anchors(region))),
        }
        Some(with(new))
    };
    Canvas::new(plane, anchor, draw, on_event).into()
}

/// The field that picks the sketch, and the line that says what of it
/// is taken (`areas` of them picked, or all of it).
pub fn sketch_fields<A: 'static>(
    sketch: &Option<SketchRef>,
    areas: usize,
    with: impl Fn(Option<SketchRef>) -> A + 'static,
) -> Vec<Widget<A>> {
    let said = if areas == 0 {
        "the whole sketch (click areas to pick)".to_owned()
    } else {
        format!("{areas} picked (click to change)")
    };
    let label =
        workpiece_core::widgets::Label::new("Areas", said, workpiece_core::widgets::Mood::Plain);
    vec![sketch_field(sketch, with), label.into()]
}

/// The box of curves of the model a step takes: edges of bodies and
/// surfaces, curve bodies.
pub fn curves_list<A: 'static>(
    curves: &[Reference<workpiece_core::Anything>],
    with: impl Fn(Vec<workpiece_core::Source>) -> A + 'static,
) -> workpiece_core::widgets::ReferenceList<A> {
    workpiece_core::widgets::ReferenceList::new(
        "Curves",
        vec![
            workpiece_core::ReferenceKind::Edge,
            workpiece_core::ReferenceKind::Body,
        ],
        Reference::sources(curves),
        with,
    )
}

/// The field that picks a direction: an axis, a straight edge, a plane or
/// a flat face (its normal); none for what the step takes by itself.
pub fn direction_input<A: 'static>(
    direction: &Option<Reference<workpiece_core::Anything>>,
    with: impl Fn(workpiece_core::Source) -> A + 'static,
    clear: impl Fn() -> A + 'static,
) -> Widget<A> {
    use workpiece_core::ReferenceKind;
    let kinds = [
        ReferenceKind::Axis,
        ReferenceKind::Edge,
        ReferenceKind::Plane,
        ReferenceKind::Face,
    ];
    ReferenceInput::of_kind(
        "Direction",
        ReferenceKind::Axis,
        direction.as_ref().map(|d| d.source.clone()),
        with,
    )
    .taking(&kinds)
    .optional("Square to the curves' plane", clear)
    .into()
}
