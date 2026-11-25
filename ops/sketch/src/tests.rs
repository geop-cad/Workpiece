use std::sync::Arc;

use workpiece_core::handles::{CanvasEvent, CanvasEventKind, CanvasView, Handle};
use workpiece_core::math::{Axis, Point2};
use workpiece_core::world::ORIGIN;
use workpiece_core::{Placement, Standalone, Widget};
use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs};

use super::*;
use crate::draw::dimension_text;

fn view() -> CanvasView {
    CanvasView {
        pixel: 0.01,
        right: Point2::X,
        up: Point2::Y,
        middle: Point2::ZERO,
    }
}

/// A sketch being edited: its arguments and the session's state.
struct Bench {
    args: SketchArgs,
    state: SketchState,
    view: CanvasView,
}

impl Bench {
    fn new() -> Self {
        let mut sketch = Sketch::new("s");
        sketch.args.plane = Some(Reference::frame_axis(ORIGIN, Axis::Z));
        Self {
            args: sketch.args,
            state: SketchState::default(),
            view: view(),
        }
    }

    fn take(&mut self, edit: Option<Edit<SketchArgs, SketchState>>) {
        if let Some(edit) = edit {
            self.args = edit.args.unwrap_or_else(|| self.args.clone());
            self.state = edit.state.unwrap_or_else(|| self.state.clone());
        }
    }

    fn event(&mut self, kind: CanvasEventKind, x: f64, y: f64) {
        let event = CanvasEvent {
            kind,
            at: Point2::new(x, y),
            add: false,
            view: self.view,
        };
        let edit = tools::handle(&self.args, &self.state, event);
        self.take(edit);
    }

    fn hover(&mut self, x: f64, y: f64) {
        self.event(CanvasEventKind::Hover, x, y);
    }

    fn click(&mut self, x: f64, y: f64) {
        self.hover(x, y);
        self.event(CanvasEventKind::Click, x, y);
    }

    fn drag(&mut self, from: (f64, f64), to: (f64, f64)) {
        self.event(CanvasEventKind::Press, from.0, from.1);
        for k in 1..=10 {
            let t = f64::from(k) / 10.0;
            let (x, y) = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            self.event(CanvasEventKind::Drag, x, y);
        }
        self.event(CanvasEventKind::Release, to.0, to.1);
    }

    fn tool(&mut self, tool: Tool) {
        let edit = tools::switch(&self.args, &self.state, tool);
        self.take(Some(edit));
    }

    fn act(&mut self, action: Action) {
        self.try_act(action)
            .unwrap_or_else(|why| panic!("{action:?} does not fit: {why}"));
    }

    /// The action, or why it was refused (the sketch stays as it was).
    fn try_act(&mut self, action: Action) -> Result<(), String> {
        let (args, selection) = modify::apply(&self.args, &self.state, action)?;
        self.args = args;
        self.state.selection = selection;
        Ok(())
    }

    fn select(&mut self, items: &[Item]) {
        self.state.selection = items.to_vec();
    }

    fn lines(&self) -> Vec<Id> {
        self.args
            .entities
            .keys()
            .copied()
            .filter(|&e| self.args.is_line(e) && !self.args.is_axis(e))
            .collect()
    }

    fn dof(&self) -> usize {
        let d = self.args.diagnose();
        assert!(d.solved, "{d:?}");
        d.dof
    }
}

#[test]
fn rectangles_are_four_level_lines_with_four_freedoms() {
    let mut b = Bench::new();
    b.tool(Tool::Rectangle);
    b.click(1.0, 1.0);
    b.click(4.0, 3.0);
    assert_eq!(b.lines().len(), 4);
    // Where it is, how wide and how high.
    assert_eq!(b.dof(), 4);
    // A width and a height, and a corner on the origin: nothing is left.
    let [bottom, right, ..] = b.lines()[..] else {
        panic!("four lines");
    };
    for (line, label) in [
        (bottom, Point2::new(2.5, 0.5)),
        (right, Point2::new(4.5, 2.0)),
    ] {
        let (kind, value, _) =
            modify::dimension_for(&b.args, &[Item::Entity(line)], label, &b.view)
                .expect("a dimension");
        b.args.dimension(kind, value, label);
    }
    assert_eq!(b.dof(), 2);
    let corner = b.args.points_of(bottom)[0];
    b.select(&[Item::Point(1), Item::Point(corner)]);
    b.act(Action::Coincident);
    assert_eq!(b.dof(), 0);
    assert!(b.args.diagnose().is_fully_constrained());
    assert!(b.args.at(corner).abs_diff_eq(Point2::ZERO, 1e-9));
    // Saying it again is one too many: refused, the sketch stays.
    b.select(&[Item::Entity(bottom)]);
    let before = b.args.clone();
    let why = b.try_act(Action::Horizontal).unwrap_err();
    assert!(why.contains("Over-defined"), "{why}");
    assert_eq!(b.args, before);
}

/// Places a dimension of `selection` with its text at `label`, as the
/// dimension tool does: its id, and what the user is told of it.
fn dimension_click(b: &mut Bench, selection: &[Item], label: (f64, f64)) -> (Id, Option<String>) {
    let at = Point2::new(label.0, label.1);
    let (kind, value, diameter) =
        modify::dimension_for(&b.args, selection, at, &b.view).expect("fits");
    let (args, id, notice) = modify::place_dimension(&b.args, kind, value, at, diameter);
    b.args = args;
    (id, notice)
}

#[test]
fn dimensions_that_overdefine_are_added_driven() {
    let mut b = Bench::new();
    b.tool(Tool::Rectangle);
    b.click(1.0, 1.0);
    b.click(4.0, 3.0);
    let [bottom, _, top, ..] = b.lines()[..] else {
        panic!("four lines");
    };
    let (id, notice) = dimension_click(&mut b, &[Item::Entity(bottom)], (2.5, 0.5));
    assert!(notice.is_none() && !b.args.is_driven(id));
    assert_eq!(b.dof(), 3);
    // The same width again, on the other side: redundant, so it is
    // added driven, saying so; it takes no freedom.
    let (driven, notice) = dimension_click(&mut b, &[Item::Entity(top)], (2.5, 3.5));
    assert!(notice.is_some_and(|n| n.contains("driven")));
    assert!(b.args.is_driven(driven));
    assert_eq!(b.dof(), 3);
    // It follows the driving one.
    assert!(b.args.set_value(id, 6.0));
    let measured = b.args.constraints[&driven]
        .dimension
        .as_ref()
        .unwrap()
        .value;
    assert!((measured - 6.0).abs() < 1e-9);
    assert_eq!(b.dof(), 3);
    assert!(b.args.diagnose().redundant.is_empty());
    // A conflicting constraint: the bottom is level; make it upright.
    b.select(&[Item::Entity(bottom)]);
    let before = b.args.clone();
    assert!(
        b.try_act(Action::Vertical)
            .unwrap_err()
            .contains("conflicts")
    );
    assert_eq!(b.args, before);
    // Driven to driving is refused, saying why, where it would
    // over-define; once the other is driven, it is fine.
    let refused = modify::set_driven(&b.args, driven, false).unwrap_err();
    assert!(refused.contains("Stays driven"), "{refused}");
    let swapped = modify::set_driven(&b.args, id, true).expect("driven is always fine");
    let swapped = modify::set_driven(&swapped, driven, false).expect("now it is the only one");
    assert_eq!(swapped.diagnose().dof, 3);
    assert!(swapped.is_driven(id) && !swapped.is_driven(driven));
}

/// A sketch with a rotation axis drawn upright through (1, -5) and
/// (1, 5), and a free point at (4, 2): the axis's id and the point's.
fn turned(b: &mut Bench) -> (Id, Id) {
    b.tool(Tool::RotationAxis);
    b.click(1.0, -5.0);
    b.click(1.0, 5.0);
    let axis = *b
        .args
        .entities
        .keys()
        .find(|&&e| b.args.is_rotation_axis(e))
        .expect("an axis");
    b.tool(Tool::Select);
    let point = b.args.add_point(Point2::new(4.0, 2.0));
    (axis, point)
}

#[test]
fn a_rotation_axis_has_three_freedoms_and_spans_the_view() {
    let mut b = Bench::new();
    let before = b.dof();
    let (axis, point) = turned(&mut b);
    b.args.points.get_mut(&point).unwrap().fixed = true;
    // A point it passes through, and its direction; drawn upright, it
    // is held upright, which takes one.
    let level = b
        .args
        .constraints
        .values()
        .filter(|c| matches!(c.kind, Kind::Parallel(a, _) if a == axis))
        .count();
    assert_eq!(level, 1);
    assert_eq!(b.dof(), before + 3 - level);
    // Without it, three.
    b.args
        .constraints
        .retain(|_, c| !matches!(c.kind, Kind::Parallel(a, _) if a == axis));
    assert_eq!(b.dof(), before + 3);
    // It is a construction line without end: across the whole view,
    // wherever that looks.
    assert!(b.args.entities[&axis].construction);
    let mut view = b.view;
    view.middle = Point2::new(250.0, 1e4);
    let (from, to) = crate::draw::axis_span(&b.args, axis, &view);
    assert!((from.x - 1.0).abs() < 1e-9 && (to.x - 1.0).abs() < 1e-9);
    let reach = 1000.0 * view.pixel;
    assert!(from.y < 1e4 - reach && to.y > 1e4 + reach, "{from} {to}");
    // Its grip turns it: dragged, it follows the pointer about the
    // point it passes through.
    let (grip, _) = crate::draw::axis_grip(&b.args, axis, &b.view).expect("a grip");
    b.drag((grip.x, grip.y), (2.0, -4.0));
    let (a, c) = b.args.line(axis).unwrap();
    assert!(a.abs_diff_eq(Point2::new(1.0, -5.0), 1e-9), "{a}");
    assert!(((c - a).angle_to(Point2::new(1.0, 1.0))).abs() < 1e-6);
}

#[test]
fn across_a_rotation_axis_is_a_diameter_and_stays_one() {
    let mut b = Bench::new();
    let (axis, point) = turned(&mut b);
    // (The axis stays where it is: the point moves.)
    let through = b.args.points_of(axis)[0];
    b.args.points.get_mut(&through).unwrap().fixed = true;
    let items = [Item::Point(point), Item::Entity(axis)];
    // Text on the point's side: a plain distance.
    let (kind, value, diameter) =
        modify::dimension_for(&b.args, &items, Point2::new(2.5, 4.0), &b.view).unwrap();
    assert!(matches!(kind, Kind::PointLine { .. }) && !diameter && (value - 3.0).abs() < 1e-9);
    // Across the axis: twice the distance, and it solves as half.
    let (id, _) = dimension_click(&mut b, &items, (-0.5, 4.0));
    let d = b.args.constraints[&id].dimension.clone().unwrap();
    assert!(d.diameter && (d.value - 6.0).abs() < 1e-9);
    assert!(b.args.set_value(id, 10.0));
    assert!((b.args.at(point).x - 6.0).abs() < 1e-9);
    // Its text dragged over to the point's side and back: a diameter
    // still, of the same value.
    b.select(&[]);
    let label = crate::draw::label_position(&b.args, id, &b.view);
    b.drag((label.x, label.y), (4.0, 4.0));
    let d = b.args.constraints[&id].dimension.clone().unwrap();
    assert!(d.diameter && (d.value - 10.0).abs() < 1e-9);
    assert!((d.label.x - 4.0).abs() < 1e-9, "{d:?}");
    b.drag((4.0, 4.0), (-3.0, 4.0));
    let d = b.args.constraints[&id].dimension.clone().unwrap();
    assert!(d.diameter && (d.value - 10.0).abs() < 1e-9);
    assert!((d.label.x + 3.0).abs() < 1e-9);
    // Only its property makes it a distance: of half the value, the
    // sketch as it is.
    b.args.set_diameter(id, false);
    assert!(b.args.solve());
    assert!((b.args.constraints[&id].dimension.as_ref().unwrap().value - 5.0).abs() < 1e-9);
    assert!((b.args.at(point).x - 6.0).abs() < 1e-9);
}

#[test]
fn dimensions_drive_and_keep_the_shape_on_its_side() {
    let mut b = Bench::new();
    b.tool(Tool::Rectangle);
    b.click(0.0, 0.0);
    b.click(4.0, 3.0);
    let right = b.lines()[1];
    let (kind, value, _) = modify::dimension_for(
        &b.args,
        &[Item::Entity(right)],
        Point2::new(5.0, 1.5),
        &b.view,
    )
    .expect("a dimension");
    // An upright line: its length, however the text is placed.
    assert!(matches!(kind, Kind::Distance(..)), "{kind:?}");
    // A slanted one, with the text beside it: its height.
    let slanted = [
        Item::Point(b.args.points_of(b.lines()[0])[0]),
        Item::Point(b.args.points_of(right)[1]),
    ];
    let (across, height, _) =
        modify::dimension_for(&b.args, &slanted, Point2::new(5.0, 1.5), &b.view)
            .expect("a dimension");
    assert!(
        matches!(
            across,
            Kind::DistanceAlong {
                axis: SketchAxis::V,
                ..
            }
        ) && (height - 3.0).abs() < 1e-9
    );
    assert!((value - 3.0).abs() < 1e-9);
    let id = b.args.dimension(kind, value, Point2::new(5.0, 1.5));
    assert!(b.args.set_value(id, 1.0));
    // The top came down; it did not flip below the bottom.
    let top = b.args.line(b.lines()[2]).expect("the top");
    assert!(
        (top.0.y - 1.0).abs() < 1e-9 && (top.1.y - 1.0).abs() < 1e-9,
        "{top:?}"
    );
    assert_eq!(dimension_text(&b.args.constraints[&id]).0, "1");
}

#[test]
fn drags_move_what_the_constraints_let_move() {
    let mut b = Bench::new();
    b.tool(Tool::Rectangle);
    b.click(1.0, 1.0);
    b.click(4.0, 3.0);
    b.tool(Tool::Select);
    // A corner follows the pointer; its neighbours stay level with it and
    // the opposite corner stays.
    b.drag((4.0, 3.0), (5.0, 4.0));
    fn at(b: &Bench, x: f64, y: f64) -> bool {
        b.args
            .points
            .values()
            .any(|p| p.at.abs_diff_eq(Point2::new(x, y), 1e-6))
    }
    assert!(at(&b, 5.0, 4.0) && at(&b, 1.0, 1.0) && at(&b, 5.0, 1.0) && at(&b, 1.0, 4.0));
    assert!(b.args.holds());
    // A line moves along with its ends.
    b.drag((3.0, 1.0), (3.0, 0.0));
    assert!(at(&b, 1.0, 0.0) && at(&b, 5.0, 0.0) && at(&b, 5.0, 4.0));
    // The fixed origin does not move.
    b.drag((0.0, 0.0), (1.0, 1.0));
    assert_eq!(b.args.at(1), Point2::ZERO);
    // A drag over nothing selects what is inside it.
    b.drag((0.5, -0.5), (5.5, 0.5));
    assert_eq!(b.state.selection.len(), 1);
}

#[test]
fn chains_switch_to_tangent_arcs_over_their_last_point() {
    let mut b = Bench::new();
    b.tool(Tool::Line);
    b.click(0.0, 0.0);
    b.click(2.0, 0.0);
    let chain = b.state.chain.expect("a chain");
    assert!(!chain.arc);
    // Away and back over the last point: an arc.
    b.hover(3.0, 0.5);
    b.hover(2.0, 0.0);
    assert!(b.state.chain.expect("a chain").arc);
    // Staying there does not switch again; leaving and coming back does.
    b.hover(2.01, 0.0);
    assert!(b.state.chain.expect("a chain").arc);
    b.hover(3.0, 0.5);
    b.hover(2.0, 0.0);
    assert!(!b.state.chain.expect("a chain").arc);
    b.hover(3.0, 0.5);
    b.hover(2.0, 0.0);
    // A half circle up to (2, 2): tangent to the line, around (2, 1).
    b.click(2.0, 2.0);
    let arc = b
        .args
        .entities
        .iter()
        .find(|(_, e)| matches!(e.shape, Shape::Arc { .. }))
        .map(|(&id, _)| id)
        .expect("an arc");
    let (_, center, radius) = b.args.circle(arc).expect("a circle");
    assert!(center.abs_diff_eq(Point2::new(2.0, 1.0), 1e-9) && (radius - 1.0).abs() < 1e-9);
    assert!(
        b.args
            .constraints
            .values()
            .any(|c| matches!(c.kind, Kind::Tangent { .. }))
    );
    // After the arc, lines again, leaving it along its tangent (−x).
    let chain = b.state.chain.expect("a chain");
    assert!(!chain.arc && chain.tangent.abs_diff_eq(-Point2::X, 1e-9));
    // The first line was drawn level: it is horizontal.
    assert!(b.args.constraints.values().any(|c| matches!(
        c.kind,
        Kind::Along {
            axis: SketchAxis::U,
            ..
        }
    )));
    // Clicking the first point closes the chain.
    b.click(0.0, 0.0);
    assert!(b.state.chain.is_none());
    assert!(b.args.holds());
}

#[test]
fn horizontal_is_what_the_view_shows_as_level() {
    let mut b = Bench::new();
    b.tool(Tool::Line);
    b.click(1.0, 1.0);
    b.click(3.0, 2.0);
    b.tool(Tool::Select);
    let line = b.lines()[0];
    // The view turned a quarter: the screen's right is the sketch's y.
    b.view.right = Point2::Y;
    b.view.up = -Point2::X;
    b.hover(9.0, 9.0);
    b.select(&[Item::Entity(line)]);
    b.act(Action::Horizontal);
    let [Item::Constraint(id)] = b.state.selection[..] else {
        panic!("a constraint");
    };
    assert!(matches!(
        b.args.constraints[&id].kind,
        Kind::Along {
            axis: SketchAxis::V,
            ..
        }
    ));
    let (p, q) = b.args.line(line).expect("the line");
    assert!(
        (p.x - q.x).abs() < 1e-9,
        "level on screen is along the sketch's y"
    );
}

#[test]
fn slots_and_polygons_keep_their_shape() {
    let mut b = Bench::new();
    b.tool(Tool::Slot);
    b.click(1.0, 1.0);
    b.click(4.0, 1.0);
    b.click(2.0, 1.5);
    // Where it is, which way it points, how long and how wide.
    assert_eq!(b.dof(), 5);
    let mut b = Bench::new();
    b.state.sides = 5;
    b.tool(Tool::Polygon);
    b.click(1.0, 1.0);
    b.click(3.0, 1.0);
    assert_eq!(b.lines().len(), 5);
    assert_eq!(b.dof(), 4);
    // Dragging a corner turns and scales it; it stays regular.
    b.tool(Tool::Select);
    b.drag((3.0, 1.0), (3.0, 2.5));
    assert!(b.args.holds());
    let lengths: Vec<f64> = b
        .lines()
        .iter()
        .map(|&l| b.args.line(l).map_or(0.0, |(p, q)| p.distance(q)))
        .collect();
    assert!(
        lengths.iter().all(|l| (l - lengths[0]).abs() < 1e-6),
        "{lengths:?}"
    );
}

#[test]
fn mirrors_and_offsets_stay_tied() {
    let mut b = Bench::new();
    b.tool(Tool::Line);
    b.click(1.0, 0.5);
    b.click(2.0, 2.0);
    b.tool(Tool::Line);
    b.click(0.0, -1.0);
    b.click(0.0, 3.0);
    b.tool(Tool::Select);
    let [line, mirror] = b.lines()[..] else {
        panic!("two lines");
    };
    b.select(&[Item::Entity(line), Item::Entity(mirror)]);
    b.act(Action::Mirror);
    let [Item::Entity(copy)] = b.state.selection[..] else {
        panic!("a copy");
    };
    let (p, q) = b.args.line(copy).expect("the copy");
    assert!(
        p.abs_diff_eq(Point2::new(-1.0, 0.5), 1e-9) && q.abs_diff_eq(Point2::new(-2.0, 2.0), 1e-9)
    );
    // The copy follows its original (with the mirror held in place).
    b.select(&[Item::Entity(mirror)]);
    b.act(Action::Fix);
    b.drag((2.0, 2.0), (3.0, 2.0));
    let (_, q) = b.args.line(copy).expect("the copy");
    assert!(q.abs_diff_eq(Point2::new(-3.0, 2.0), 1e-6), "{q}");

    // A rectangle offset: four parallel lines sharing corners, one distance.
    let mut b = Bench::new();
    b.tool(Tool::Rectangle);
    b.click(0.0, 0.0);
    b.click(4.0, 2.0);
    b.tool(Tool::Select);
    let lines: Vec<Item> = b.lines().into_iter().map(Item::Entity).collect();
    b.select(&lines);
    b.act(Action::Offset);
    assert_eq!(b.lines().len(), 8);
    assert!(b.args.holds());
    let shown: Vec<Id> = b
        .args
        .constraints
        .iter()
        .filter(|(id, c)| matches!(c.kind, Kind::Offset { .. }) && b.args.shows(**id))
        .map(|(&id, _)| id)
        .collect();
    assert_eq!(shown.len(), 1, "one distance for the whole offset");
    // The rectangle runs counter-clockwise, so its left is inside.
    assert!(b.args.set_value(shown[0], 0.5));
    let Item::Entity(inner) = b.state.selection[0] else {
        panic!("a copy");
    };
    let (p, q) = b.args.line(inner).expect("the inner bottom");
    let (_, outer) = b.args.line(b.lines()[0]).expect("the outer bottom");
    assert!(p.abs_diff_eq(Point2::new(0.5, 0.5), 1e-9), "{p}");
    assert!(
        q.abs_diff_eq(outer + Point2::new(-0.5, 0.5), 1e-9),
        "{q} {outer}"
    );
}

fn boxed() -> anyhow::Result<Standalone> {
    let mut b = Standalone::new();
    AddBox {
        id: "box".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum(ORIGIN)),
            size: [2.0, 4.0, 6.0].map(workpiece_core::Measure::new),
        },
    }
    .apply(&mut b)?;
    Ok(b)
}

#[test]
fn sketches_leave_profiles_with_tolerances_and_cache_their_solution() -> anyhow::Result<()> {
    let mut b = Bench::new();
    b.tool(Tool::Circle);
    b.click(0.0, 0.0);
    b.click(1.0, 0.0);
    b.tool(Tool::Line);
    b.click(2.0, 0.0);
    b.click(3.0, 0.0);
    b.click(3.0, 1.0);
    let sketch = Sketch {
        id: "s".into(),
        args: b.args.clone(),
    };
    let mut builder = Standalone::new();
    sketch.apply(&mut builder)?;
    let profile = &builder.world.profiles["s"];
    // The circle and two lines; the origin point is construction.
    assert_eq!(profile.edges.len(), 3);
    assert!(profile.is_closed_within_tolerance());
    assert!(
        profile
            .edges
            .iter()
            .all(|e| e.tolerance >= model::DEFAULT_TOLERANCE)
    );
    let circle = profile
        .edges
        .iter()
        .find(|e| e.start == e.end)
        .expect("the circle");
    assert!((circle.curve.point_at(0.3).length() - 1.0).abs() < 1e-12);
    // The solution is cached under the sketch's id, and shown from there.
    assert!(builder.cache.get("s/args").is_some());
    assert_eq!(sketch.shown(&builder.cache), sketch.args);
    // A sketch cannot be applied twice, and needs a plane.
    assert!(sketch.apply(&mut builder).is_err());
    assert!(Sketch::new("t").apply(&mut builder).is_err());
    Ok(())
}

#[test]
fn projections_are_what_the_edges_are_and_follow_the_model() -> anyhow::Result<()> {
    let builder = boxed()?;
    let world = Arc::new(builder.world.clone());
    let mut sketch = Sketch::from_selection("s", &world, &[Source::Topology("box/top".into())]);
    assert!(sketch.args.plane.is_some());
    // Project the top face through the ribbon's box: its four edges, as
    // lines.
    let cache = Cache::default();
    let context = EditContext::standalone(&world, &cache);
    let edit = sketch.visualize_edit(&context, &SketchState::default());
    assert_eq!(edit.placement, Placement::Ribbon);
    let mut list = None;
    edit.dialog.visit(&mut |w| {
        if let Widget::ReferenceList(l) = w {
            list = Some(l);
        }
    });
    let list = list.expect("the projection box");
    let top = Source::Topology("box/top".into());
    assert!(list.accepts(&world, &top));
    sketch.args = list.toggle(top).args.expect("new arguments");
    let projected: Vec<Id> = sketch
        .args
        .entities
        .keys()
        .copied()
        .filter(|&e| sketch.args.is_projected(e))
        .collect();
    assert_eq!(projected.len(), 4);
    assert!(projected.iter().all(|&e| sketch.args.is_line(e)));
    // Converted, a projected edge leaves a line of the sketch's own
    // where it is: free, and not the model's.
    let state = SketchState {
        selection: vec![Item::Entity(projected[0])],
        ..SketchState::default()
    };
    let (converted, own) =
        modify::apply(&sketch.args, &state, Action::Detach).expect("it converts");
    let [Item::Entity(own)] = own[..] else {
        panic!("a line");
    };
    assert!(!converted.is_projected(own) && converted.is_projected(projected[0]));
    assert_eq!(converted.line(own), sketch.args.line(projected[0]));
    assert!(
        converted
            .points_of(own)
            .iter()
            .all(|p| !converted.points[p].fixed)
    );
    // The longest projected edge.
    let reach = |args: &SketchArgs| {
        projected
            .iter()
            .filter_map(|&e| args.line(e))
            .map(|(p, q)| p.distance(q))
            .fold(0.0, f64::max)
    };
    assert!((reach(&sketch.args) - 4.0).abs() < 1e-9);
    // A circle tangent to a projected edge, with a fixed radius.
    let mut args = sketch.args.clone();
    let center = args.add_point(Point2::new(1.0, 1.0));
    let circle = args.add(Shape::Circle {
        center,
        radius: 0.5,
    });
    args.constrain(Kind::Tangent {
        a: projected[0],
        b: circle,
        internal: false,
    });
    args.dimension(Kind::Size(circle), 0.5, Point2::ZERO);
    assert!(args.solve());
    sketch.args = args;
    let touching = |args: &SketchArgs| {
        let (a, b) = args.line(projected[0]).expect("the edge");
        let (_, c, r) = args.circle(circle).expect("the circle");
        ((b - a).normalize().perp_dot(c - a).abs() - r).abs() < 1e-9
    };
    assert!(touching(&sketch.args));

    // The box grows: the projection moves, the circle follows, and the
    // stored arguments stay as they are; the cache has the new solution.
    let mut bigger = Standalone::new();
    AddBox {
        id: "box".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum(ORIGIN)),
            size: [6.0, 8.0, 6.0].map(workpiece_core::Measure::new),
        },
    }
    .apply(&mut bigger)?;
    sketch.apply(&mut bigger)?;
    let shown = sketch.shown(&bigger.cache);
    assert_ne!(shown, sketch.args);
    // The cache holds what was solved; projecting again gives the rest.
    let (_, solved) = sketch.solved(&bigger.world, &bigger.cache, &bigger.parameters)?;
    assert!((reach(&solved) - 8.0).abs() < 1e-9);
    assert!(touching(&solved));
    let profile = &bigger.world.profiles["s"];
    // The circle alone: the projected edges are a reference, which the
    // sketch's areas are not made of.
    assert_eq!(profile.edges.len(), 1);
    assert!(profile.is_closed_within_tolerance());
    // An edge that is gone is an error, not a guess.
    assert!(sketch.apply(&mut Standalone::new()).is_err());
    Ok(())
}

#[test]
fn the_menu_offers_what_fits_the_selection() {
    let mut b = Bench::new();
    b.tool(Tool::Line);
    b.click(0.0, 1.0);
    b.click(2.0, 1.2);
    b.tool(Tool::Line);
    b.click(0.0, 2.0);
    b.click(2.0, 2.5);
    b.tool(Tool::Select);
    let lines: Vec<Item> = b.lines().into_iter().map(Item::Entity).collect();
    b.select(&lines);
    let sketch = Sketch {
        id: "s".into(),
        args: b.args.clone(),
    };
    let world = Arc::new(World::new());
    let cache = Cache::default();
    let context = EditContext::standalone(&world, &cache);
    let edit = sketch.visualize_edit(&context, &b.state);
    let offered: Vec<&str> = edit.menu.iter().map(|t| t.label.as_str()).collect();
    for wanted in [
        "Dimension",
        "Parallel",
        "Perpendicular",
        "Equal",
        "Horizontal",
        "Delete",
    ] {
        assert!(offered.contains(&wanted), "{wanted} in {offered:?}");
    }
    assert!(!offered.contains(&"Tangent") && !offered.contains(&"Midpoint"));
    // In the ribbon, what does not fit says why.
    let mut tangent = None;
    edit.dialog.visit(&mut |w| {
        if let Widget::ToolButton(t) = w
            && t.label == "Tangent"
        {
            tangent = t.disabled.clone();
        }
    });
    assert_eq!(
        tangent.as_deref(),
        Some("Select a line and a circle or arc, or two circles or arcs")
    );
    // Every button has its own hotkey.
    let mut keys = Vec::new();
    edit.dialog.visit(&mut |w| {
        if let Widget::ToolButton(t) = w {
            keys.push(t.hotkey);
        }
    });
    let mut unique = keys.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), keys.len());
    // The canvas is the sketch's one handle.
    assert!(matches!(
        edit.handles.get("sketch"),
        Some(Handle::Canvas(_))
    ));
    // Choosing parallel makes them so.
    let parallel = edit
        .menu
        .iter()
        .find(|t| t.label == "Parallel")
        .expect("parallel");
    let args = parallel
        .click()
        .and_then(|e| e.args)
        .expect("new arguments");
    let dir = |l: &Item| match l {
        Item::Entity(e) => args.line(*e).map(|(p, q)| (q - p).normalize()),
        _ => None,
    };
    let (Some(d1), Some(d2)) = (dir(&lines[0]), dir(&lines[1])) else {
        panic!("two lines");
    };
    assert!(d1.perp_dot(d2).abs() < 1e-9);
}

#[test]
fn dimensions_read_like_a_drawing_and_sketches_round_trip() -> anyhow::Result<()> {
    let mut b = Bench::new();
    b.tool(Tool::Circle);
    b.click(1.0, 1.0);
    b.click(2.0, 1.0);
    let circle = *b.args.entities.keys().last().expect("the circle");
    let id = b
        .args
        .dimension_of(Kind::Size(circle), 2.0, Point2::new(3.0, 3.0), true);
    let text = |args: &SketchArgs| dimension_text(&args.constraints[&id]);
    assert_eq!(text(&b.args).0, "Ø2");
    let mut set = |tolerance| {
        b.args
            .constraints
            .get_mut(&id)
            .and_then(|c| c.dimension.as_mut())
            .expect("a dimension")
            .tolerance = tolerance;
        text(&b.args)
    };
    assert_eq!(set(DimTolerance::Fit("H7".into())).0, "Ø2 H7");
    assert_eq!(set(DimTolerance::Symmetric(0.05)).0, "Ø2 ±0.05");
    let (main, upper, lower) = set(DimTolerance::Deviations {
        upper: 0.2,
        lower: -0.1,
    });
    assert_eq!(
        (main.as_str(), upper.as_str(), lower.as_str()),
        ("Ø2", "+0.2", "−0.1")
    );
    // The shapes: a line across with two arrowheads, and the text.
    let shapes = draw::shapes(&b.args, &b.state, &b.args.diagnose(), &b.view);
    let arrows = shapes
        .iter()
        .filter(|s| matches!(s, workpiece_core::handles::CanvasShape::Arrowhead { .. }))
        .count();
    assert_eq!(arrows, 2);

    let sketch = Sketch {
        id: "s".into(),
        args: b.args.clone(),
    };
    let text = serde_json::to_string(&sketch)?;
    assert_eq!(serde_json::from_str::<Sketch>(&text)?, sketch);
    Ok(())
}

#[test]
fn dimensions_take_formulas_over_the_parts_parameters() -> anyhow::Result<()> {
    let mut b = Bench::new();
    b.tool(Tool::Rectangle);
    b.click(0.0, 0.0);
    b.click(4.0, 3.0);
    let bottom = b.lines()[0];
    let (kind, value, _) = modify::dimension_for(
        &b.args,
        &[Item::Entity(bottom)],
        Point2::new(2.0, -1.0),
        &b.view,
    )
    .expect("a dimension");
    let id = b.args.dimension(kind, value, Point2::new(2.0, -1.0));
    b.args
        .constraints
        .get_mut(&id)
        .and_then(|c| c.dimension.as_mut())
        .expect("a dimension")
        .formula = Some("width / 2".into());
    let sketch = Sketch {
        id: "s".into(),
        args: b.args.clone(),
    };
    // Each build values the formula with the parameters it is built with:
    // a variant of the part is as wide as its own width says.
    for width in [8.0, 3.0] {
        let mut builder = Standalone::new();
        builder.parameters.insert("width".into(), width);
        sketch.apply(&mut builder)?;
        let profile = &builder.world.profiles["s"];
        let edge = profile
            .edges
            .iter()
            .find(|e| e.name == format!("e{bottom}"))
            .expect("the bottom");
        let length = edge
            .curve
            .point_at(edge.t0)
            .distance(edge.curve.point_at(edge.t1));
        assert!((length - width / 2.0).abs() < 1e-9, "{length} for {width}");
    }
    // Without the parameter, the build says what is missing.
    let error = sketch.apply(&mut Standalone::new()).expect_err("no width");
    assert!(
        format!("{error:#}").contains("no parameter \"width\""),
        "{error:#}"
    );
    Ok(())
}

#[test]
fn regions_are_named_by_their_boundaries() -> anyhow::Result<()> {
    use workpiece_core::world::{AreaAnchor, AreaError, DatumPlane};
    let profile_of = |args: &SketchArgs| profile::profile(args, DatumPlane::xy());

    // A rectangle with a hole: the plate around the hole, and the disk.
    let mut b = Bench::new();
    b.tool(Tool::Rectangle);
    b.click(1.0, 1.0);
    b.click(5.0, 4.0);
    b.tool(Tool::Circle);
    b.click(3.0, 2.5);
    b.click(4.0, 2.5);
    let profile = profile_of(&b.args);
    assert_eq!(profile.edges.len(), 5);
    assert_eq!(profile.regions.len(), 2);
    let disk = std::f64::consts::PI;
    let mut areas: Vec<f64> = profile.regions.iter().map(|r| r.area).collect();
    areas.sort_by(f64::total_cmp);
    assert!(
        (areas[0] - disk).abs() < 0.02 && (areas[1] - (12.0 - disk)).abs() < 0.02,
        "{areas:?}"
    );
    let plate = profile
        .regions
        .iter()
        .position(|r| r.holes.len() == 1)
        .expect("the plate");
    // Its anchors: its four sides from the inside, the circle from outside.
    let anchors = profile.anchors(plate);
    assert_eq!(anchors.len(), 5);
    assert_eq!(profile.region_at(&anchors), Ok(plate));
    assert_eq!(profile.region_at(&[]), Err(AreaError::NoAnchors));

    // The rectangle grows: the same anchors still name the plate.
    b.tool(Tool::Select);
    b.drag((5.0, 4.0), (7.0, 5.0));
    let grown = profile_of(&b.args);
    let region = grown.region_at(&anchors)?;
    assert!((grown.regions[region].area - (24.0 - disk)).abs() < 0.02);

    // A line across the plate splits it: the anchors no longer agree.
    b.tool(Tool::Line);
    b.click(0.5, 4.5);
    b.click(7.5, 4.5);
    let split = profile_of(&b.args);
    assert_eq!(split.regions.len(), 3);
    assert_eq!(split.region_at(&anchors), Err(AreaError::Split(2)));
    // The crossing is a vertex named after the two curves; the pieces of
    // the cut side are named after their ends.
    assert!(
        split.vertices.iter().any(|v| v.name.starts_with("x(e")),
        "{:?}",
        split.vertices
    );
    assert!(
        split
            .edges
            .iter()
            .any(|e| e.name.contains("[") && e.span.1 < 1.0)
    );
    assert!(split.is_closed_within_tolerance());
    // The disk, named by the inside of the circle, is still the disk.
    let inside = profile.anchors(1 - plate);
    assert_eq!(inside.len(), 1);
    let disk_now = split.region_at(&inside)?;
    assert!((split.regions[disk_now].area - disk).abs() < 0.02);
    // An anchor on a curve that is gone names nothing.
    let gone = AreaAnchor {
        entity: "e999".into(),
        left: true,
        at: 0.5,
    };
    assert!(matches!(
        split.region_at(&[gone]),
        Err(AreaError::Lost { .. })
    ));

    // Two circles that overlap: the lens and the two crescents, each
    // bounded by both circles and told apart by the sides.
    let mut b = Bench::new();
    b.tool(Tool::Circle);
    b.click(1.0, 3.0);
    b.click(3.0, 3.0);
    b.tool(Tool::Circle);
    b.click(3.5, 3.0);
    b.click(5.5, 3.0);
    let profile = profile_of(&b.args);
    assert_eq!(profile.regions.len(), 3);
    for region in 0..3 {
        assert_eq!(profile.region_at(&profile.anchors(region)), Ok(region));
    }
    Ok(())
}

/// A plate with a big round side (R97) and a small round (R12) in its top
/// corner, tangent to the side and to the top: the main area and the
/// sliver between the round, the side and the top. `split`: the side and
/// the top are cut where the round meets them (the curves end there);
/// otherwise the round's ends are only held on them (a T on each). The
/// round's ends are put `off` along the curves from the true tangent
/// points, as a solver leaves them (a point on two tangent curves is only
/// known to the root of what the solve leaves over). `mirror`: the same
/// the other way round.
fn tangent_corner(split: bool, off: f64, mirror: bool) -> (SketchArgs, [Id; 2]) {
    let s = if mirror { -1.0 } else { 1.0 };
    let (big, small) = (97.0_f64, 12.0_f64);
    let c_big = Point2::new(90.0, 30.0);
    let side_x = c_big.x - (big * big - 30.0 * 30.0).sqrt();
    let c_small = Point2::new(c_big.x - ((big - small).powi(2) - 18.0 * 18.0).sqrt(), 48.0);
    let touch = c_big + (c_small - c_big) * (big / (big - small));
    let at = |p: Point2| Point2::new(s * p.x, p.y);
    let mut args = SketchArgs::default();
    let mut point = |p: Point2| args.add_point(at(p));
    let (cb, cs) = (point(c_big), point(c_small));
    let (a, b, c, d) = (
        point(Point2::new(side_x, 0.0)),
        point(Point2::new(60.0, 0.0)),
        point(Point2::new(60.0, 60.0)),
        point(Point2::new(side_x, 60.0)),
    );
    // The round's ends, a little off: along the top, and around the side.
    let e = point(Point2::new(c_small.x + off, 60.0));
    let turn = off / big;
    let t =
        point(c_big + crate::geometry::unit(crate::geometry::angle(touch - c_big) - turn) * big);
    // An arc counter-clockwise from `from` to `to` (mirrored: the other
    // way, so it is the same curve).
    let arc = |args: &mut SketchArgs, center, from, to| {
        let (start, end) = if mirror { (to, from) } else { (from, to) };
        args.add(Shape::Arc { center, start, end })
    };
    let bottom = args.add_line(a, b);
    args.add_line(b, c);
    let round = arc(&mut args, cs, e, t);
    let top_near_side;
    if split {
        args.add_line(c, e);
        top_near_side = args.add_line(e, d);
        arc(&mut args, cb, d, t);
        arc(&mut args, cb, t, a);
    } else {
        top_near_side = args.add_line(c, d);
        let side = arc(&mut args, cb, d, a);
        args.constrain(Kind::PointOn {
            point: e,
            entity: top_near_side,
        });
        args.constrain(Kind::PointOn {
            point: t,
            entity: side,
        });
    }
    let _ = round;
    (args, [bottom, top_near_side])
}

#[test]
fn areas_are_traced_where_curves_meet_tangent() -> anyhow::Result<()> {
    use workpiece_core::world::{AreaAnchor, DatumPlane};
    // The sliver's area, and the whole plate's.
    let (sliver, whole) = (16.84, 3924.36);
    for split in [true, false] {
        for off in [0.0, 1e-4, -1e-4] {
            for mirror in [false, true] {
                let (args, [bottom, top]) = tangent_corner(split, off, mirror);
                let profile = profile::profile(&args, DatumPlane::xy());
                let what = format!("split {split}, off {off}, mirror {mirror}");
                assert_eq!(profile.regions.len(), 2, "{what}: {:?}", profile.regions);
                let mut areas: Vec<f64> = profile.regions.iter().map(|r| r.area).collect();
                areas.sort_by(f64::total_cmp);
                assert!(
                    (areas[0] - sliver).abs() < 0.05 && (areas[1] - (whole - sliver)).abs() < 0.2,
                    "{what}: {areas:?}"
                );
                for region in 0..2 {
                    assert_eq!(
                        profile.region_at(&profile.anchors(region)),
                        Ok(region),
                        "{what}"
                    );
                }
                // The plate, by its bottom; the sliver, by the top beside
                // the side (below it).
                let inside = |entity: Id, at: f64| AreaAnchor {
                    entity: format!("e{entity}"),
                    left: !mirror,
                    at,
                };
                let (main, small) = (
                    profile.region_at(&[inside(bottom, 0.5)])?,
                    profile.region_at(&[inside(top, if split { 0.5 } else { 0.98 })])?,
                );
                assert!(
                    (profile.regions[main].area - (whole - sliver)).abs() < 0.2,
                    "{what}"
                );
                assert!(
                    (profile.regions[small].area - sliver).abs() < 0.05,
                    "{what}"
                );
                assert!(profile.is_closed_within_tolerance(), "{what}");
            }
        }
    }
    Ok(())
}

#[test]
fn curves_leaving_a_point_tangent_are_ordered_by_their_bend() {
    use std::f64::consts::PI;
    use workpiece_core::world::DatumPlane;
    // A big round (R97) through the origin, upright there, in two pieces:
    // a long one up, one down. A short small round (R12) leaves the origin
    // upright too, tangent to both, bending sooner to the right. The first
    // steps of the curves' outlines would put the long piece's bend after
    // the short round's: only how far each bends a like distance out
    // orders them right.
    let (cb, big, cs, small) = (Point2::new(97.0, 0.0), 97.0, Point2::new(12.0, 0.0), 12.0);
    let on = |c: Point2, r: f64, a: f64| c + crate::geometry::unit(a) * r;
    let mut args = SketchArgs::default();
    let o = args.add_point(Point2::ZERO);
    let (c_big, c_small) = (args.add_point(cb), args.add_point(cs));
    let low = args.add_point(on(cb, big, PI + 0.5));
    let up = args.add_point(on(cb, big, PI - 1.0));
    let k = args.add_point(on(cs, small, PI - 0.1));
    args.add(Shape::Arc {
        center: c_big,
        start: o,
        end: low,
    });
    args.add(Shape::Arc {
        center: c_big,
        start: up,
        end: o,
    });
    args.add(Shape::Arc {
        center: c_small,
        start: k,
        end: o,
    });
    let across = args.add_line(k, up);
    args.add_line(low, up);
    let profile = profile::profile(&args, DatumPlane::xy());
    assert_eq!(profile.regions.len(), 2, "{:?}", profile.regions);
    // The sliver between the long piece and the small round, closed by
    // the line across: by a fine outline.
    let shoelace = |points: &[Point2]| -> f64 {
        (0..points.len())
            .map(|i| points[i].perp_dot(points[(i + 1) % points.len()]) / 2.0)
            .sum()
    };
    let n = 4000;
    let sliver: Vec<Point2> = (0..=n)
        .map(|i| on(cb, big, PI - 1.0 + i as f64 / n as f64))
        .chain((0..=n).map(|i| on(cs, small, PI - 0.1 * i as f64 / n as f64)))
        .collect();
    let whole: Vec<Point2> = (0..=n)
        .map(|i| on(cb, big, PI - 1.0 + 1.5 * i as f64 / n as f64))
        .collect();
    let (sliver, whole) = (shoelace(&sliver), shoelace(&whole));
    let anchor = |left| workpiece_core::world::AreaAnchor {
        entity: format!("e{across}"),
        left,
        at: 0.5,
    };
    let (a, b) = (
        profile
            .region_at(&[anchor(true)])
            .expect("left of the line across"),
        profile.region_at(&[anchor(false)]).expect("right of it"),
    );
    let (a, b) = (profile.regions[a].area, profile.regions[b].area);
    assert!((a - sliver).abs() < 0.01 * sliver, "{a} {sliver}");
    assert!((b - (whole - sliver)).abs() < 0.01 * whole, "{b} {whole}");
}

/// The canvas of the sketch `b` is editing, as its edit shows it.
fn canvas_of(b: &Bench) -> workpiece_core::handles::Canvas<Edit<SketchArgs, SketchState>> {
    let sketch = Sketch {
        id: "s".into(),
        args: b.args.clone(),
    };
    let world = Arc::new(World::new());
    let cache = Cache::default();
    let context = EditContext::standalone(&world, &cache);
    let edit = sketch.visualize_edit(&context, &b.state);
    edit.handles
        .into_values()
        .find_map(|h| match h {
            Handle::Canvas(canvas) => Some(canvas),
            _ => None,
        })
        .expect("a canvas")
}

fn texts(b: &Bench, tone: workpiece_core::handles::Tone) -> Vec<String> {
    use workpiece_core::handles::CanvasShape;
    canvas_of(b)
        .shapes(&b.view)
        .into_iter()
        .filter_map(|s| match s {
            CanvasShape::Text { text, tone: t, .. } if t == tone => Some(text),
            _ => None,
        })
        .collect()
}

/// The panel at the side of the sketch being edited, if it shows.
fn panel_of(b: &Bench) -> Option<Widget<Edit<SketchArgs, SketchState>>> {
    let sketch = Sketch {
        id: "s".into(),
        args: b.args.clone(),
    };
    let world = Arc::new(World::new());
    let cache = Cache::default();
    let context = EditContext::standalone(&world, &cache);
    sketch.visualize_edit(&context, &b.state).inspector
}

/// The field of the panel captioned `label`.
fn field<'a, M: 'static>(panel: &'a Widget<M>, label: &str) -> Option<&'a Widget<M>> {
    panel.children().iter().find(|w| match w {
        Widget::NumericInput(n) => n.label == label,
        Widget::Check(c) => c.label == label,
        Widget::Label(l) => l.label == label,
        _ => false,
    })
}

#[test]
fn dimensions_preview_where_they_go_and_take_their_value_in_their_panel() {
    use workpiece_core::handles::Tone;
    let mut b = Bench::new();
    b.tool(Tool::Rectangle);
    b.click(0.0, 0.0);
    b.click(4.0, 3.0);
    let bottom = b.lines()[0];
    b.select(&[]);
    b.tool(Tool::Dimension);
    b.click(2.0, 0.0);
    assert_eq!(b.state.selection, [Item::Entity(bottom)]);
    // Over nothing, the dimension shows where the pointer would put it.
    b.hover(2.0, -1.0);
    assert_eq!(texts(&b, Tone::Preview), ["4"]);
    // Over something else to pick, it does not.
    b.hover(4.0, 1.5);
    assert!(texts(&b, Tone::Preview).is_empty());
    // Placed, its panel shows, its value field taking the keyboard.
    b.click(2.0, -1.0);
    let [Item::Constraint(id)] = b.state.selection[..] else {
        panic!("{:?}", b.state.selection);
    };
    assert_eq!(b.state.focus, Some(id));
    let panel = panel_of(&b).expect("a panel");
    let Some(Widget::NumericInput(value)) = field(&panel, "Value") else {
        panic!("a value field");
    };
    assert!(value.focus.is_some() && (value.value - 4.0).abs() < 1e-12);
    assert!(matches!(field(&panel, "Driven"), Some(Widget::Check(c)) if !c.value));
    // (A straight distance is no diameter.)
    assert!(field(&panel, "Diameter").is_none());
    // What is not a size is not taken, and says why.
    let refused = value.enter("-2").expect("an edit");
    assert!(refused.args.is_none() && refused.state.unwrap().notice.is_some());
    // A number is: the sketch follows, the keyboard is given back.
    b.take(value.enter("6"));
    assert_eq!(b.state.focus, None);
    let (from, to) = b.args.line(bottom).expect("the bottom");
    assert!((from.distance(to) - 6.0).abs() < 1e-9);
    // A double click gives it the keyboard again; a click elsewhere
    // takes it, and with nothing selected the panel goes.
    b.tool(Tool::Select);
    b.event(CanvasEventKind::DoubleClick, 2.0, -1.0);
    assert_eq!(b.state.focus, Some(id));
    b.click(8.0, 8.0);
    assert_eq!(b.state.focus, None);
    assert!(panel_of(&b).is_none());
}

#[test]
fn a_value_that_conflicts_is_not_taken_and_driven_ones_have_no_field() {
    let mut b = Bench::new();
    // Three points: the sides of a triangle; the third side cannot be
    // longer than the other two together.
    let pts: Vec<Id> = [(0.0, 0.0), (4.0, 0.0), (4.0, 3.0)]
        .iter()
        .map(|&(x, y)| b.args.add_point(Point2::new(x, y)))
        .collect();
    b.args.points.get_mut(&pts[0]).unwrap().fixed = true;
    let mut ids = Vec::new();
    for (k, at) in [(0, 1), (1, 2), (2, 0)].into_iter().enumerate() {
        let kind = Kind::Distance(pts[at.0], pts[at.1]);
        let value = b.args.at(pts[at.0]).distance(b.args.at(pts[at.1]));
        let (args, id, notice) =
            modify::place_dimension(&b.args, kind, value, Point2::splat(k as f64), false);
        assert!(notice.is_none());
        b.args = args;
        ids.push(id);
    }
    b.select(&[Item::Constraint(ids[2])]);
    let before = b.args.clone();
    let panel = panel_of(&b).expect("a panel");
    let Some(Widget::NumericInput(value)) = field(&panel, "Value") else {
        panic!("a value field");
    };
    let refused = value.enter("20").expect("an edit");
    assert!(refused.args.is_none() && refused.state.unwrap().notice.is_some());
    assert_eq!(b.args, before);
    // Driven: what it measures, and no field to type in.
    b.args.set_driven(ids[2], true);
    let panel = panel_of(&b).expect("a panel");
    assert!(field(&panel, "Value").is_none());
    assert!(matches!(field(&panel, "Measures"), Some(Widget::Label(l)) if l.text == "(5)"));
}

#[test]
fn tools_show_what_they_would_snap_to_before_the_first_click() {
    use workpiece_core::handles::{CanvasShape, Tone};
    let mut b = Bench::new();
    b.tool(Tool::Line);
    b.click(0.0, 0.0);
    b.click(4.0, 0.0);
    b.tool(Tool::Line);
    let line = b.lines()[0];
    let end = b.args.points_of(line)[1];
    // Near the line's end: the end lights up, the pointer's dot sits on
    // it, and a badge says the points will coincide.
    b.hover(4.03, 0.04);
    assert_eq!(b.state.hover, Some(Item::Point(end)));
    let shapes = canvas_of(&b).shapes(&b.view);
    let dot = shapes.iter().any(|s| {
        matches!(s, CanvasShape::Dot { at, tone: Tone::Preview, .. }
            if at.abs_diff_eq(Point2::new(4.0, 0.0), 1e-12))
    });
    assert!(dot);
    assert_eq!(texts(&b, Tone::Preview), ["◎"]);
    // Near the line: onto it; near its middle: there.
    b.hover(1.0, 0.05);
    assert_eq!(b.state.hover, Some(Item::Entity(line)));
    assert_eq!(texts(&b, Tone::Preview), ["on"]);
    b.hover(2.03, 0.05);
    let cursor = b.state.cursor.expect("a cursor");
    assert!(cursor.middle && cursor.at.abs_diff_eq(Point2::new(2.0, 0.0), 1e-12));
    assert_eq!(texts(&b, Tone::Preview), ["M"]);
    // Away from everything: just the dot.
    b.hover(2.0, 2.0);
    assert_eq!(b.state.hover, None);
    assert!(texts(&b, Tone::Preview).is_empty());
    // A circle's second click only takes the position: no badge.
    b.tool(Tool::Circle);
    b.click(2.0, 2.0);
    b.hover(4.03, 0.04);
    assert!(texts(&b, Tone::Preview).is_empty());
}

#[test]
fn sketches_start_with_what_lies_in_their_plane() -> anyhow::Result<()> {
    let builder = boxed()?;
    let world = Arc::new(builder.world.clone());
    // On the top face: its four edges come along, as fixed lines (their
    // ends with them), and nothing else of the box.
    let sketch = Sketch::from_selection("s", &world, &[Source::Topology("box/top".into())]);
    let auto: Vec<Id> = sketch
        .args
        .entities
        .iter()
        .filter(|(_, e)| e.auto)
        .map(|(&id, _)| id)
        .collect();
    assert_eq!(auto.len(), 4);
    assert!(
        auto.iter()
            .all(|&e| sketch.args.is_line(e) && sketch.args.is_projected(e))
    );
    // It builds; where the box is gone, what nothing was built on goes
    // with it instead of failing the sketch for its plane alone.
    let mut again = boxed()?;
    sketch.apply(&mut again)?;
    assert!(again.world.profiles["s"].is_closed_within_tolerance());
    let mut on_origin = sketch.clone();
    on_origin.args.plane = Some(Reference::frame_axis(ORIGIN, Axis::Z));
    let mut empty = Standalone::new();
    on_origin.apply(&mut empty)?;
    assert!(empty.world.profiles["s"].edges.is_empty());
    Ok(())
}

#[test]
fn tools_snap_to_crossings_and_draw_construction_geometry() {
    use workpiece_core::handles::Tone;
    let mut b = Bench::new();
    b.tool(Tool::Line);
    b.click(0.0, 0.0);
    b.click(4.0, 4.0);
    b.tool(Tool::Line);
    b.click(0.0, 4.0);
    b.click(4.0, 0.0);
    let crossed = b.lines();
    // Near where the two lines cross, a new line starts on both.
    b.tool(Tool::Line);
    b.hover(2.03, 2.05);
    let cursor = b.state.cursor.expect("a cursor");
    assert!(cursor.at.abs_diff_eq(Point2::new(2.0, 2.0), 1e-9));
    assert_eq!(texts(&b, Tone::Preview), ["×"]);
    // In construction mode, what is drawn is construction geometry.
    b.state.construction = true;
    b.click(2.03, 2.05);
    b.click(2.0, 6.0);
    let new: Vec<Id> = b
        .lines()
        .into_iter()
        .filter(|l| !crossed.contains(l))
        .collect();
    let [line] = new[..] else {
        panic!("{new:?}");
    };
    assert!(b.args.entities[&line].construction);
    let start = b.args.points_of(line)[0];
    let on: Vec<Id> = b
        .args
        .constraints
        .values()
        .filter_map(|c| match c.kind {
            Kind::PointOn { point, entity } if point == start => Some(entity),
            _ => None,
        })
        .collect();
    assert_eq!(on.len(), 2);
    assert!(crossed.iter().all(|l| on.contains(l)));
}

#[test]
fn the_planes_axes_are_construction_lines_without_end() {
    use workpiece_core::handles::Tone;
    let mut b = Bench::new();
    let axis = |b: &Bench, which: SketchAxis| {
        let found = b.args.entities.iter().find(|(_, e)| e.axis == Some(which));
        *found.expect("an axis").0
    };
    let (u, v) = (axis(&b, SketchAxis::U), axis(&b, SketchAxis::V));
    // Far out along it, a line still starts on the axis.
    b.tool(Tool::Line);
    b.hover(37.0, 0.04);
    assert_eq!(b.state.hover, Some(Item::Entity(u)));
    assert_eq!(texts(&b, Tone::Preview), ["on"]);
    b.click(37.0, 0.04);
    b.click(40.0, 3.0);
    let line = b.lines()[0];
    let start = b.args.points_of(line)[0];
    assert!(b.args.at(start).abs_diff_eq(Point2::new(37.0, 0.0), 1e-9));
    assert!(b.args.constraints.values().any(
        |c| matches!(c.kind, Kind::PointOn { point, entity } if point == start && entity == u)
    ));
    // Its angle to the axis is a dimension; the axis alone has none.
    b.tool(Tool::Select);
    b.click(-20.0, 0.0);
    assert_eq!(b.state.selection, [Item::Entity(u)]);
    assert!(modify::no_dimension(&b.args, &b.state).is_some());
    b.select(&[Item::Entity(u), Item::Entity(line)]);
    let (kind, value, _) =
        modify::dimension_for(&b.args, &b.state.selection, Point2::new(39.0, 1.0), &b.view)
            .expect("an angle");
    assert!(matches!(kind, Kind::Angle(..)), "{kind:?}");
    assert!((value - 45.0).abs() < 1e-9, "{value}");
    // Where the axes cross is the origin; the points that only give
    // them their direction are not there to pick.
    b.tool(Tool::Point);
    b.hover(1.0, 0.0);
    assert_eq!(b.state.hover, Some(Item::Entity(u)));
    // They stay: not deleted, not made part of the profile, not moved.
    b.tool(Tool::Select);
    b.select(&[Item::Entity(u), Item::Entity(v)]);
    b.act(Action::Delete);
    assert!(b.args.is_axis(u) && b.args.is_axis(v));
    assert!(modify::unavailable(&b.args, &b.state, Action::Fix).is_some());
    b.drag((-5.0, 0.0), (-5.0, 3.0));
    assert_eq!(b.args.line(u), Some((Point2::ZERO, Point2::X)));
}

#[test]
fn chains_go_straight_on_from_arcs_and_points_align_with_hovered_ones() {
    use workpiece_core::handles::Tone;
    let mut b = Bench::new();
    // A line, a tangent arc (hover back over the end), then a line
    // nearly straight on from the arc: it is made tangent to it.
    b.tool(Tool::Line);
    b.click(1.0, 1.0);
    b.click(3.0, 1.0);
    b.hover(4.0, 1.5);
    b.hover(3.0, 1.0);
    assert!(b.state.chain.is_some_and(|c| c.arc));
    b.click(4.0, 2.0);
    let chain = b.state.chain.expect("a chain");
    let arc = chain.previous;
    assert!(b.args.circle(arc).is_some());
    // The arc leaves upwards.
    assert!(
        chain.tangent.abs_diff_eq(Point2::Y, 1e-9),
        "{:?}",
        chain.tangent
    );
    b.hover(4.05, 4.0);
    let cursor = b.state.cursor.expect("a cursor");
    assert!(cursor.tangent && cursor.at.abs_diff_eq(Point2::new(4.0, 4.0), 1e-9));
    assert_eq!(texts(&b, Tone::Preview), ["T"]);
    b.click(4.05, 4.0);
    let line = b.state.chain.expect("a chain").previous;
    assert!(b.args.constraints.values().any(|c| matches!(
        c.kind,
        Kind::Tangent { a, b, .. } if (a, b) == (line, arc) || (a, b) == (arc, line)
    )));
    b.event(CanvasEventKind::DoubleClick, 4.05, 4.0);

    // A point hovered is remembered: a new point snaps level with it,
    // and is tied to it.
    let first = b.args.points_of(b.lines()[0])[0];
    b.tool(Tool::Point);
    b.hover(1.0, 1.0);
    assert_eq!(b.state.remembered, [first]);
    b.hover(1.04, 6.0);
    let cursor = b.state.cursor.expect("a cursor");
    assert_eq!(cursor.aligned, Some((first, SketchAxis::V)));
    assert!(cursor.at.abs_diff_eq(Point2::new(1.0, 6.0), 1e-9));
    assert_eq!(texts(&b, Tone::Preview), ["V"]);
    b.click(1.04, 6.0);
    assert!(b.args.constraints.values().any(|c| matches!(
        c.kind,
        Kind::Along { a, axis: SketchAxis::V, .. } if a == first
    )));
    // Away from it, nothing.
    b.hover(2.0, 6.5);
    assert_eq!(b.state.cursor.and_then(|c| c.aligned), None);
}

#[test]
fn points_align_with_two_hovered_ones_and_splines_close_on_their_start() {
    let mut b = Bench::new();
    // Two points; a third is put level with the one and upright with
    // the other: where the two cross, tied to both.
    b.tool(Tool::Point);
    b.click(2.0, 1.0);
    b.click(6.0, 4.0);
    let at = |b: &Bench, x: f64, y: f64| {
        let found =
            b.args.points.iter().find(|(id, p)| {
                !b.args.is_axis_end(**id) && p.at.abs_diff_eq(Point2::new(x, y), 1e-9)
            });
        *found.expect("a point there").0
    };
    let (first, second) = (at(&b, 2.0, 1.0), at(&b, 6.0, 4.0));
    b.hover(2.0, 1.0);
    b.hover(6.0, 4.0);
    assert_eq!(b.state.remembered, [second, first]);
    b.hover(2.05, 3.96);
    let cursor = b.state.cursor.expect("a cursor");
    assert!(
        cursor.at.abs_diff_eq(Point2::new(2.0, 4.0), 1e-9),
        "{:?}",
        cursor.at
    );
    let both = [cursor.aligned, cursor.aligned_too].map(|a| a.expect("level with both"));
    assert!(both.contains(&(second, SketchAxis::U)) && both.contains(&(first, SketchAxis::V)));
    b.click(2.05, 3.96);
    let third = at(&b, 2.0, 4.0);
    for (other, axis) in [(second, SketchAxis::U), (first, SketchAxis::V)] {
        assert!(b.args.constraints.values().any(|c| matches!(
            c.kind,
            Kind::Along { a, b: tied, axis: along } if (a, tied, along) == (other, third, axis)
        )));
    }

    // A spline whose last click is back on its first is a loop, and
    // done: the two ends are one place.
    let mut b = Bench::new();
    b.tool(Tool::Spline);
    b.click(0.0, 5.0);
    b.click(3.0, 8.0);
    b.click(6.0, 5.0);
    b.hover(0.04, 5.03);
    let cursor = b.state.cursor.expect("a cursor");
    assert!(cursor.at.abs_diff_eq(Point2::new(0.0, 5.0), 1e-9));
    b.click(0.04, 5.03);
    assert!(b.state.clicks.is_empty());
    let (control, ..) = b
        .args
        .entities
        .values()
        .find_map(|e| match &e.shape {
            Shape::Spline { control, .. } => Some((control.clone(),)),
            _ => None,
        })
        .expect("a spline");
    assert_eq!(control.len(), 4);
    assert!(b.args.constraints.values().any(|c| matches!(
        c.kind,
        Kind::Coincident(a, b) if (a, b) == (control[3], control[0]) || (a, b) == (control[0], control[3])
    )));
    assert!(
        b.args
            .at(control[0])
            .abs_diff_eq(b.args.at(control[3]), 1e-9)
    );
}

#[test]
fn with_a_grid_free_clicks_land_on_it() {
    use workpiece_core::handles::CanvasShape;
    let mut b = Bench::new();
    // At this zoom the grid's lines are 0.2 apart.
    assert!((tools::grid_step(&b.view) - 0.2).abs() < 1e-12);
    let lines = |b: &Bench| {
        let shapes = canvas_of(b).shapes(&b.view);
        let thin =
            |s: &&CanvasShape| matches!(s, CanvasShape::Polyline { width, .. } if *width < 0.55);
        shapes.iter().filter(thin).count()
    };
    assert_eq!(lines(&b), 0);
    b.state.grid = true;
    assert!(lines(&b) > 20);
    // A click free of anything else lands on the grid; one near a point
    // still on the point.
    b.tool(Tool::Point);
    b.hover(3.07, 2.96);
    let cursor = b.state.cursor.expect("a cursor");
    assert!(
        cursor.at.abs_diff_eq(Point2::new(3.0, 3.0), 1e-9),
        "{:?}",
        cursor.at
    );
    b.click(3.07, 2.96);
    b.click(5.33, 1.11);
    assert!(
        b.args
            .points
            .values()
            .any(|p| p.at.abs_diff_eq(Point2::new(5.4, 1.2), 1e-9))
    );
    b.hover(3.02, 3.03);
    assert!(matches!(
        b.state.cursor.and_then(|c| c.to),
        Some(Item::Point(_))
    ));
}

#[test]
fn the_grid_holds_along_a_level_and_for_shapes_and_splines_change_degree() {
    // A line drawn level lands on the grid along its level; so do a
    // rectangle's corners.
    let mut b = Bench::new();
    b.state.grid = true;
    b.tool(Tool::Line);
    b.click(1.03, 1.04);
    b.hover(3.27, 1.02);
    let cursor = b.state.cursor.expect("a cursor");
    assert!(cursor.along.is_some());
    assert!(
        cursor.at.abs_diff_eq(Point2::new(3.2, 1.0), 1e-9),
        "{:?}",
        cursor.at
    );
    b.click(3.27, 1.02);
    b.event(CanvasEventKind::DoubleClick, 3.27, 1.02);
    let line = b.lines()[0];
    assert_eq!(
        b.args.line(line),
        Some((Point2::new(1.0, 1.0), Point2::new(3.2, 1.0)))
    );
    b.tool(Tool::Rectangle);
    b.click(5.07, 5.11);
    b.click(7.93, 6.49);
    let corner = |x: f64, y: f64| {
        b.args
            .points
            .values()
            .any(|p| p.at.abs_diff_eq(Point2::new(x, y), 1e-9))
    };
    assert!(corner(5.0, 5.2) && corner(8.0, 6.4) && corner(5.0, 6.4) && corner(8.0, 5.2));

    // A spline is of the degree asked for; and of another, once that is
    // set for it.
    let mut b = Bench::new();
    b.tool(Tool::Spline);
    b.state.degree = 2;
    for (x, y) in [(0.0, 5.0), (2.0, 7.0), (4.0, 5.0), (6.0, 7.0), (8.0, 5.0)] {
        b.click(x, y);
    }
    b.event(CanvasEventKind::DoubleClick, 8.0, 5.0);
    let degree_of = |b: &Bench| {
        b.args.entities.iter().find_map(|(id, e)| match &e.shape {
            Shape::Spline { degree, .. } => Some((*id, *degree)),
            _ => None,
        })
    };
    let (spline, degree) = degree_of(&b).expect("a spline");
    assert_eq!(degree, 2);
    b.select(&[Item::Entity(spline)]);
    let sketch = Sketch {
        id: "s".into(),
        args: b.args.clone(),
    };
    let world = Arc::new(World::new());
    let cache = Cache::default();
    let context = EditContext::standalone(&world, &cache);
    let shown = sketch.visualize_edit(&context, &b.state);
    let mut edit = None;
    // (The degree of the spline selected is in its panel.)
    shown.inspector.expect("a panel").visit(&mut |w| {
        if let Widget::NumericInput(field) = w
            && field.label == "Degree"
        {
            assert_eq!(field.value, 2.0);
            edit = Some((field.on_change)(4.0));
        }
    });
    assert!(edit.is_some(), "a degree field");
    b.take(edit);
    assert_eq!(degree_of(&b).map(|d| d.1), Some(4));
}

#[test]
fn circles_by_two_and_three_points_and_ellipses() {
    let circle = |b: &Bench| {
        let found = b.args.entities.keys().find_map(|&e| b.args.circle(e));
        found.expect("a circle")
    };
    let mut b = Bench::new();
    b.tool(Tool::Circle2);
    b.click(1.0, 1.0);
    b.click(5.0, 1.0);
    let (_, center, radius) = circle(&b);
    assert!(center.abs_diff_eq(Point2::new(3.0, 1.0), 1e-9) && (radius - 2.0).abs() < 1e-9);

    let mut b = Bench::new();
    b.tool(Tool::Circle3);
    b.click(2.0, 0.5);
    b.click(0.5, 2.0);
    b.click(-1.0, 0.5);
    let (_, center, radius) = circle(&b);
    assert!(center.abs_diff_eq(Point2::new(0.5, 0.5), 1e-9) && (radius - 1.5).abs() < 1e-9);

    // An ellipse 3 by 1 about (1, 1): one closed area of π·3·1.
    let mut b = Bench::new();
    b.tool(Tool::Ellipse);
    b.click(1.0, 1.0);
    b.click(4.0, 1.0);
    b.click(2.0, 2.0);
    let profile = crate::profile::profile(&b.args, workpiece_core::world::DatumPlane::xy());
    assert_eq!(profile.regions.len(), 1);
    assert!(
        (profile.regions[0].area - 3.0 * std::f64::consts::PI).abs() < 0.05,
        "{}",
        profile.regions[0].area
    );
    let edge = &profile.edges[0];
    let on = edge.curve.point_at((edge.t0 + edge.t1) / 8.0) - Point2::new(1.0, 1.0);
    assert!(
        ((on.x / 3.0).powi(2) + on.y.powi(2) - 1.0).abs() < 1e-9,
        "{on}"
    );
}

#[test]
fn corners_are_rounded_cut_off_split_and_run_on() {
    let rectangle = || {
        let mut b = Bench::new();
        b.tool(Tool::Rectangle);
        b.click(0.0, 0.0);
        b.click(4.0, 2.0);
        b.tool(Tool::Select);
        b
    };
    let corner = |b: &Bench, at: Point2| {
        let found = b
            .args
            .points
            .iter()
            .find(|(_, p)| p.at.abs_diff_eq(at, 1e-9));
        *found.expect("the corner").0
    };
    // A corner rounded: an arc tangent to its two sides, which end on it.
    let mut b = rectangle();
    let c = corner(&b, Point2::new(4.0, 2.0));
    b.select(&[Item::Point(c)]);
    b.act(Action::Fillet);
    let [Item::Entity(arc)] = b.state.selection[..] else {
        panic!("an arc");
    };
    let (_, center, radius) = b.args.circle(arc).expect("the arc");
    assert!(
        center.abs_diff_eq(Point2::new(4.0 - radius, 2.0 - radius), 1e-6),
        "{center}"
    );
    assert!(b.args.holds());
    assert_eq!(b.lines().len(), 4);
    // Cut off: five lines around.
    let mut b = rectangle();
    let c = corner(&b, Point2::new(4.0, 0.0));
    b.select(&[Item::Point(c)]);
    b.act(Action::Chamfer);
    assert_eq!(b.lines().len(), 5);
    let [Item::Entity(cut)] = b.state.selection[..] else {
        panic!("a line");
    };
    let (p, q) = b.args.line(cut).expect("the cut");
    assert!((p.x - p.y - (q.x - q.y)).abs() < 1e-9 && p.distance(q) > 0.01);

    // A line split at a point on it, and a line run on to another.
    let mut b = Bench::new();
    b.tool(Tool::Line);
    b.click(0.0, 0.0);
    b.click(4.0, 0.0);
    b.tool(Tool::Line);
    b.click(6.0, -1.0);
    b.click(6.0, 3.0);
    b.tool(Tool::Point);
    b.click(1.0, 0.0);
    b.tool(Tool::Select);
    let [first, wall] = b.lines()[..] else {
        panic!("two lines");
    };
    let at = corner(&b, Point2::new(1.0, 0.0));
    b.select(&[Item::Entity(first), Item::Point(at)]);
    b.act(Action::Split);
    assert_eq!(b.lines().len(), 3);
    let far = *b.lines().last().expect("the other half");
    b.select(&[Item::Entity(far), Item::Entity(wall)]);
    b.act(Action::Extend);
    let (p, q) = b.args.line(far).expect("the half");
    assert!(
        p.abs_diff_eq(Point2::new(1.0, 0.0), 1e-6) && q.abs_diff_eq(Point2::new(6.0, 0.0), 1e-6),
        "{p} {q}"
    );
}

#[test]
fn curves_are_copied_and_repeated() {
    // A circle and a step line; then three of it along the line, and
    // four around a point.
    let mut b = Bench::new();
    b.tool(Tool::Circle);
    b.click(1.0, 1.0);
    b.click(1.5, 1.0);
    b.tool(Tool::Line);
    b.click(0.0, -1.0);
    b.click(2.0, -1.0);
    b.tool(Tool::Point);
    b.click(1.0, 3.0);
    b.tool(Tool::Select);
    let circles = |b: &Bench| -> Vec<Point2> {
        let all = b.args.entities.keys().filter_map(|&e| b.args.circle(e));
        all.map(|c| c.1).collect()
    };
    let circle = *b
        .args
        .entities
        .iter()
        .find(|(_, e)| matches!(e.shape, Shape::Circle { .. }))
        .expect("the circle")
        .0;
    let [step] = b.lines()[..] else {
        panic!("a line");
    };
    let mut linear = Bench {
        args: b.args.clone(),
        state: b.state.clone(),
        view: b.view,
    };
    linear.select(&[Item::Entity(circle), Item::Entity(step)]);
    linear.act(Action::LinearPattern);
    let found = circles(&linear);
    assert_eq!(found.len(), 3);
    assert!(
        found
            .iter()
            .any(|c| c.abs_diff_eq(Point2::new(5.0, 1.0), 1e-9)),
        "{found:?}"
    );
    // Around (1, 3), with a count of four.
    let centre = *b
        .args
        .points
        .iter()
        .find(|(_, p)| p.at.abs_diff_eq(Point2::new(1.0, 3.0), 1e-9))
        .expect("the point")
        .0;
    b.state.copies = 4;
    b.select(&[Item::Entity(circle), Item::Point(centre)]);
    b.act(Action::CircularPattern);
    let found = circles(&b);
    assert_eq!(found.len(), 4);
    for at in [[3.0, 3.0], [1.0, 5.0], [-1.0, 3.0]] {
        assert!(
            found
                .iter()
                .any(|c| c.abs_diff_eq(Point2::from_array(at), 1e-9)),
            "{found:?}"
        );
    }
    // A copy, to be dragged.
    b.select(&[Item::Entity(circle)]);
    b.act(Action::Copy);
    assert_eq!(circles(&b).len(), 5);
    // A slanted line (no constraint holds it level) turned a quarter
    // turn about the point, then made twice as large from it.
    b.tool(Tool::Line);
    b.click(8.0, 8.0);
    b.click(9.0, 10.0);
    b.tool(Tool::Select);
    let slanted = *b.lines().last().expect("the line");
    let ends = |b: &Bench| b.args.line(slanted).expect("the line");
    b.state.angle = 90.0;
    b.select(&[Item::Entity(slanted), Item::Point(centre)]);
    b.act(Action::Rotate);
    let (p, q) = ends(&b);
    assert!(
        p.abs_diff_eq(Point2::new(-4.0, 10.0), 1e-9)
            && q.abs_diff_eq(Point2::new(-6.0, 11.0), 1e-9),
        "{p} {q}"
    );
    b.select(&[Item::Entity(slanted), Item::Point(centre)]);
    b.act(Action::Scale);
    let (p, q) = ends(&b);
    assert!(
        p.abs_diff_eq(Point2::new(-9.0, 17.0), 1e-9)
            && q.abs_diff_eq(Point2::new(-13.0, 19.0), 1e-9),
        "{p} {q}"
    );
}

#[test]
fn splines_go_through_their_clicks_when_asked() {
    let mut b = Bench::new();
    b.tool(Tool::Spline);
    b.state.through = true;
    let clicks = [(1.0, 1.0), (3.0, 2.5), (5.0, 1.5), (7.0, 3.0), (9.0, 1.0)];
    for (x, y) in clicks {
        b.click(x, y);
    }
    b.tool(Tool::Select);
    let profile = crate::profile::profile(&b.args, workpiece_core::world::DatumPlane::xy());
    let edge = profile.edges.first().expect("the spline");
    // Every click is on the curve.
    for (x, y) in clicks {
        let at = Point2::new(x, y);
        let near = (0..=2000)
            .map(|k| {
                edge.curve
                    .point_at(edge.t0 + (edge.t1 - edge.t0) * f64::from(k) / 2000.0)
            })
            .map(|p| p.distance(at))
            .fold(f64::INFINITY, f64::min);
        assert!(near < 5e-3, "{at} is {near} off");
    }
}

#[test]
fn the_plane_cuts_the_model_into_curves_of_the_sketch() -> anyhow::Result<()> {
    // The box (4 × 6 × 4, on the origin) cut by the plane y = 0: a
    // square outline of four lines.
    let builder = boxed()?;
    use workpiece_core::math::Point3;
    let plane = workpiece_core::world::DatumPlane::new(Point3::ZERO, Point3::Y, Point3::X)?;
    let found = crate::project::sections(&builder.world, &plane);
    assert_eq!(found.len(), 4);
    let mut long = 0.0;
    for f in &found {
        let workpiece_core::world::topo::nurbs::Recognized::Line(a, b) = f.shape else {
            panic!("a line");
        };
        long += a.distance(b);
    }
    let size = {
        let topo = &builder.world.topo;
        let body = topo.bodies().ids().next().expect("the box");
        let solid = workpiece_boolean::read(topo, body)?;
        let (lo, hi) = solid.extent();
        hi - lo
    };
    assert!((long - 2.0 * (size.x + size.z)).abs() < 1e-9, "{long}");
    Ok(())
}

#[test]
fn what_the_plane_cuts_is_offered_but_not_drawn_until_projected() -> anyhow::Result<()> {
    // The box cut through its middle by the sketch plane offers four
    // lines to snap to and pick; none of them is drawn.
    let builder = boxed()?;
    let world = Arc::new(builder.world.clone());
    let plane = Source::FrameAxis {
        frame: "origin".into(),
        axis: Axis::Y,
    };
    let sketch = Sketch::from_selection("s", &world, &[plane]);
    let offered: Vec<Id> = sketch
        .args
        .entities
        .iter()
        .filter(|(_, e)| e.section)
        .map(|(&id, _)| id)
        .collect();
    assert_eq!(offered.len(), 4);
    assert!(offered.iter().all(|&id| {
        let e = &sketch.args.entities[&id];
        e.auto && e.construction && sketch.args.is_line(id) && e.projected.is_none()
    }));
    let drawn = |args: &SketchArgs, hover: Option<Item>| {
        let state = SketchState {
            hover,
            ..SketchState::default()
        };
        let diagnosis = args.diagnose();
        crate::draw::shapes(args, &state, &diagnosis, &view()).len()
    };
    let hidden = drawn(&sketch.args, None);
    // Pointed at, one shows.
    assert!(drawn(&sketch.args, Some(Item::Entity(offered[0]))) > hidden);
    // Projected, all four show, as references.
    let mut args = sketch.args.clone();
    for e in args.entities.values_mut() {
        if e.section {
            e.auto = false;
            e.section = false;
        }
    }
    assert!(drawn(&args, None) >= hidden + 4);
    // Selected, the Project button projects what is offered; there is
    // no button that cuts the model.
    let state = SketchState {
        selection: vec![Item::Entity(offered[0])],
        ..SketchState::default()
    };
    let cache = Cache::default();
    let context = EditContext::standalone(&world, &cache);
    let shown = sketch.visualize_edit(&context, &state);
    let mut project = None;
    shown.dialog.visit(&mut |w| {
        if let Widget::ToolButton(tool) = w {
            assert_ne!(tool.label, "Intersect");
            if tool.label == "Project" {
                assert!(tool.large);
                project = tool.click();
            }
        }
    });
    let projected = project.expect("a Project button").args.expect("projected");
    assert!(!projected.entities[&offered[0]].auto);
    // They are still offered where the model is the same: refreshed,
    // nothing is offered twice.
    let again = Sketch::from_selection(
        "s",
        &world,
        &[Source::FrameAxis {
            frame: "origin".into(),
            axis: Axis::Y,
        }],
    );
    assert_eq!(
        again.args.entities.values().filter(|e| e.section).count(),
        4
    );
    Ok(())
}

#[test]
fn text_is_written_as_closed_outlines() -> anyhow::Result<()> {
    use workpiece_core::world::topo::nurbs::Recognized;
    // An "I" 2 high is one outline, from the baseline to 2 up; an "O"
    // is two (its outside and its hole), of curves.
    let ends = |shape: &Recognized| match shape {
        Recognized::Line(a, b) => (*a, *b),
        Recognized::Spline(c) => {
            let (from, to) = c.domain();
            (c.point_at(from), c.point_at(to))
        }
        _ => panic!("a line or a curve"),
    };
    let outlines = |text: &str| -> anyhow::Result<usize> {
        let shapes = crate::text::outlines(text, 2.0, Point2::new(1.0, 1.0))?;
        // Each piece starts where the one before ends, or a new outline
        // starts — which ends where it started.
        let (mut count, mut start, mut at) = (0, Point2::ZERO, Point2::ZERO);
        for (k, shape) in shapes.iter().enumerate() {
            let (a, b) = ends(shape);
            if k == 0 || a.distance(at) > 1e-9 {
                assert!(
                    k == 0 || at.distance(start) < 1e-9,
                    "an outline is left open"
                );
                count += 1;
                start = a;
            }
            at = b;
        }
        assert!(at.distance(start) < 1e-9, "the last outline is left open");
        let (lo, hi) = shapes
            .iter()
            .map(ends)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (a, b)| {
                (lo.min(a.y).min(b.y), hi.max(a.y).max(b.y))
            });
        assert!(
            (lo - 1.0).abs() < 0.05 && (hi - 3.0).abs() < 0.05,
            "{lo} {hi}"
        );
        Ok(count)
    };
    assert_eq!(outlines("I")?, 1);
    assert_eq!(outlines("O")?, 2);
    assert!(crate::text::outlines("I", 0.0, Point2::ZERO).is_err());
    Ok(())
}

/// The height of what `args`' text `id` writes: from its lowest
/// outline to its highest, along the box's up.
fn text_rows(args: &SketchArgs, id: crate::Id) -> (f64, f64) {
    use workpiece_core::world::topo::nurbs::Recognized;
    let frame = args.text_frame(id).expect("a box");
    let ys = args.text_outlines(id).into_iter().flat_map(|s| match s {
        Recognized::Line(a, b) => vec![a, b],
        Recognized::Spline(c) => c.control_points().to_vec(),
        _ => Vec::new(),
    });
    ys.map(|p| (p - frame.origin).dot(frame.up))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), y| {
            (lo.min(y), hi.max(y))
        })
}

#[test]
fn text_is_written_in_a_box_that_turns_wraps_and_aligns() -> anyhow::Result<()> {
    // Three clicks: along its baseline from (1, 1) to (9, 1), then 3
    // up. The box's corners are points with its sides between them.
    let mut b = Bench::new();
    b.state.text = "IO".into();
    b.tool(Tool::Text);
    b.click(1.0, 1.0);
    b.click(9.0, 1.0);
    b.click(5.0, 4.0);
    let text = *b
        .args
        .entities
        .iter()
        .find(|(_, e)| matches!(e.shape, Shape::Text(_)))
        .expect("a text")
        .0;
    let Some(Shape::Text(boxed)) = b.args.entities.get(&text).map(|e| e.shape.clone()) else {
        panic!("a text");
    };
    assert_eq!(boxed.text, "IO");
    // Its capitals half as high as the box, to begin with; here 1.
    assert!((boxed.size - 1.5).abs() < 1e-12, "{}", boxed.size);
    if let Some(Shape::Text(t)) = b.args.entities.get_mut(&text).map(|e| &mut e.shape) {
        t.size = 1.0;
    }
    let frame = b.args.text_frame(text).expect("its box");
    assert!(frame.origin.abs_diff_eq(Point2::new(1.0, 1.0), 1e-9));
    assert!((frame.width - 8.0).abs() < 1e-9 && (frame.height - 3.0).abs() < 1e-9);
    // At the top of the box: the capitals' tops a little under it.
    let (_, top) = text_rows(&b.args, text);
    assert!(top < 3.0 && top > 2.0, "{top}");
    // The letters are areas of the profile: the "I", and the "O" with
    // the hole in it; the box is not.
    let sketch = Sketch {
        id: "s".into(),
        args: b.args.clone(),
    };
    let mut builder = Standalone::new();
    sketch.apply(&mut builder)?;
    let profile = &builder.world.profiles["s"];
    assert!(profile.is_closed_within_tolerance());
    assert_eq!(profile.regions.len(), 3);
    assert!(
        profile
            .edges
            .iter()
            .all(|e| e.entity.starts_with(&format!("e{text}/c")))
    );
    // Drawn level, it is held level; let go of that, its corner
    // dragged turns the box, and the letters with it.
    let level: Vec<crate::Id> = b
        .args
        .constraints
        .iter()
        .filter(|(_, c)| matches!(c.kind, Kind::Along { .. }))
        .map(|(&id, _)| id)
        .collect();
    assert_eq!(level.len(), 1);
    b.args.constraints.remove(&level[0]);
    let corner = boxed.corners[1];
    b.tool(Tool::Select);
    b.drag((9.0, 1.0), (1.0 + 8.0 * 0.8, 1.0 + 8.0 * 0.6));
    let frame = b.args.text_frame(text).expect("its box");
    assert!(b.args.at(corner).distance(Point2::new(7.4, 5.8)) < 1e-3);
    assert!(frame.along.y > 0.3, "turned: {:?}", frame.along);
    assert!((frame.up.dot(frame.along)).abs() < 1e-9, "still square");
    // Words that do not fit the box's width break onto lines of their
    // own; without breaking, they stand on one line, past the box.
    let set = |b: &mut Bench, f: &dyn Fn(&mut crate::TextBox)| {
        if let Some(Shape::Text(t)) = b.args.entities.get_mut(&text).map(|e| &mut e.shape) {
            f(t);
        }
    };
    set(&mut b, &|t| t.text = "IIIII IIIII IIIII IIIII".into());
    let (low, high) = text_rows(&b.args, text);
    assert!(high - low > 2.0, "on lines of their own: {low} {high}");
    set(&mut b, &|t| t.wrap = false);
    let (low, high) = text_rows(&b.args, text);
    assert!((high - low - 1.0).abs() < 0.05, "one line: {low} {high}");
    // A typed line break breaks it all the same; at the bottom of the
    // box, the last line stands on it.
    set(&mut b, &|t| {
        t.text = "I\nI".into();
        t.valign = crate::Align::End;
    });
    let (low, high) = text_rows(&b.args, text);
    assert!(low > 0.0 && low < 0.5 && high - low > 2.0, "{low} {high}");
    // Centred, a letter stands in the middle of the line.
    set(&mut b, &|t| {
        t.text = "I".into();
        t.align = crate::Align::Middle;
    });
    let frame = b.args.text_frame(text).expect("its box");
    use workpiece_core::world::topo::nurbs::Recognized;
    let xs: Vec<f64> = b
        .args
        .text_outlines(text)
        .into_iter()
        .flat_map(|s| match s {
            Recognized::Line(p, q) => vec![p, q],
            _ => Vec::new(),
        })
        .map(|p| (p - frame.origin).dot(frame.along))
        .collect();
    let middle = (xs.iter().copied().fold(f64::INFINITY, f64::min)
        + xs.iter().copied().fold(f64::NEG_INFINITY, f64::max))
        / 2.0;
    assert!((middle - frame.width / 2.0).abs() < 0.1, "{middle}");
    // Deleted, its box goes with it.
    b.state.selection = vec![Item::Entity(text)];
    let (gone, _) = modify::apply(&b.args, &b.state, Action::Delete).expect("deleted");
    b.args = gone;
    assert!(
        b.args
            .entities
            .values()
            .all(|e| e.axis.is_some() || matches!(e.shape, Shape::Point(_)))
    );
    Ok(())
}

#[test]
fn a_turned_rectangle_is_four_square_sides_by_three_clicks() {
    let mut b = Bench::new();
    b.tool(Tool::RotatedRectangle);
    b.click(1.0, 1.0);
    b.click(4.0, 5.0);
    assert!(
        b.state.selection.is_empty(),
        "two clicks are no rectangle yet"
    );
    // To the left of the first side, 2 across.
    b.click(1.0 - 1.6, 1.0 + 1.2);
    // (Its sides in the order drawn.)
    let lines: Vec<crate::Id> = b.lines();
    assert_eq!(lines.len(), 4);
    let length = |b: &Bench, k: usize| {
        let (p, q) = b.args.line(lines[k]).expect("a line");
        p.distance(q)
    };
    assert!((length(&b, 0) - 5.0).abs() < 1e-9 && (length(&b, 1) - 2.0).abs() < 1e-9);
    // Held square: one corner dragged, it stays a rectangle.
    b.tool(Tool::Select);
    let (_, far) = b.args.line(lines[1]).expect("a line");
    b.drag((far.x, far.y), (far.x - 1.0, far.y + 0.5));
    for k in 0..4 {
        let (a, c) = b.args.line(lines[k]).expect("a line");
        let (_, d) = b.args.line(lines[(k + 1) % 4]).expect("a line");
        assert!((c - a).dot(d - c).abs() < 1e-6, "square at corner {k}");
    }
    // Five degrees of freedom: where, how turned, how long, how wide.
    assert_eq!(b.args.diagnose().dof, 5);
}

#[test]
fn splines_run_on_with_the_curvature_of_what_they_end_on() {
    // A Bézier of degree 3 from the end of a line, and one from a
    // point of a circle of radius 2: held smooth, each leaves along
    // what it ends on, straight from the line and bent as the circle.
    let spline = |args: &mut SketchArgs, from: crate::Id, rest: [(f64, f64); 3]| {
        let mut control = vec![from];
        control.extend(rest.map(|(x, y)| args.add_point(Point2::new(x, y))));
        args.add(Shape::Spline {
            control,
            degree: 3,
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            weights: vec![1.0; 4],
        })
    };
    // (Its curvature where it starts, to the left of its way.)
    let bend = |args: &SketchArgs, id: crate::Id| {
        let p: Vec<Point2> = args.points_of(id).iter().map(|&p| args.at(p)).collect();
        let t = p[1] - p[0];
        2.0 / 3.0 * t.perp_dot(p[2] - p[0]) / t.length().powi(3)
    };
    let mut args = Bench::new().args;
    let (a, b) = (
        args.add_point(Point2::new(0.0, 0.0)),
        args.add_point(Point2::new(2.0, 0.0)),
    );
    let line = args.add(Shape::Line(a, b));
    for p in [a, b] {
        args.points.get_mut(&p).expect("the point").fixed = true;
    }
    let from_line = spline(&mut args, b, [(3.0, 0.5), (4.0, 1.5), (5.0, 3.0)]);
    args.constrain(Kind::Smooth {
        spline: from_line,
        other: line,
    });
    let center = args.add_point(Point2::new(0.0, 10.0));
    let circle = args.add(Shape::Circle {
        center,
        radius: 2.0,
    });
    args.dimension(Kind::Size(circle), 2.0, Point2::ZERO);
    let on = args.add_point(Point2::new(2.0, 10.0));
    for p in [center, on] {
        args.points.get_mut(&p).expect("the point").fixed = true;
    }
    let from_circle = spline(&mut args, on, [(2.5, 11.0), (2.0, 12.5), (0.5, 13.5)]);
    args.constrain(Kind::Smooth {
        spline: from_circle,
        other: circle,
    });
    assert!(args.solve());
    let along: Vec<Point2> = args
        .points_of(from_line)
        .iter()
        .map(|&p| args.at(p))
        .collect();
    assert!(
        along[1].y.abs() < 1e-9 && along[2].y.abs() < 1e-9,
        "{along:?}"
    );
    assert!(along[1].x > 2.0, "it goes on, not back");
    let around: Vec<Point2> = args
        .points_of(from_circle)
        .iter()
        .map(|&p| args.at(p))
        .collect();
    assert!(
        (around[1].x - 2.0).abs() < 1e-9,
        "square to the radius: {around:?}"
    );
    assert!(
        (bend(&args, from_circle).abs() - 0.5).abs() < 1e-9,
        "{}",
        bend(&args, from_circle)
    );
    // (To the side the centre is on: the left, going up.)
    assert!(bend(&args, from_circle) > 0.0);
}

#[test]
fn what_cannot_hold_is_named() {
    // Two points held where they are, 1 apart, said to be 5 apart: no
    // constraint depends on another, yet that one does not hold.
    let mut args = SketchArgs::default();
    let (a, b) = (
        args.add_point(Point2::ZERO),
        args.add_point(Point2::new(1.0, 0.0)),
    );
    for p in [a, b] {
        args.points.get_mut(&p).expect("just added").fixed = true;
    }
    let said = args.dimension(Kind::Distance(a, b), 5.0, Point2::ZERO);
    let d = args.diagnose();
    assert!(!d.solved);
    assert!(d.conflicting.contains(&said), "{d:?}");
}

#[test]
fn round_things_are_dimensioned_to_their_sides_and_about_an_axis() {
    // Two circles on the u axis, the second 10 to the right.
    let mut b = Bench::new();
    let a = &mut b.args;
    let origin = a
        .points
        .iter()
        .find(|(_, p)| p.fixed && p.at == Point2::ZERO)
        .map(|(id, _)| *id)
        .expect("the origin");
    let far = a.add_point(Point2::new(9.0, 0.5));
    let (c1, c2) = (
        a.add(Shape::Circle {
            center: origin,
            radius: 2.5,
        }),
        a.add(Shape::Circle {
            center: far,
            radius: 3.5,
        }),
    );
    let u = a
        .entities
        .iter()
        .find(|(_, e)| e.axis == Some(SketchAxis::U))
        .map(|(id, _)| *id)
        .expect("the u axis");
    let v = a
        .entities
        .iter()
        .find(|(_, e)| e.axis == Some(SketchAxis::V))
        .map(|(id, _)| *id)
        .expect("the v axis");
    a.constrain(Kind::PointOn {
        point: far,
        entity: u,
    });
    a.dimension(Kind::Size(c1), 2.0, Point2::ZERO);
    a.dimension(Kind::Size(c2), 3.0, Point2::ZERO);
    // The gap between them 5: their middles are 2 + 5 + 3 apart.
    a.dimension(Kind::Gap(c1, c2), 5.0, Point2::ZERO);
    assert!(a.solve());
    assert!((a.at(far).x - 10.0).abs() < 1e-9, "{:?}", a.at(far));
    assert_eq!(a.diagnose().dof, 0);
    // From the v axis to the far side of the second: 13, and for
    // reference only — it holds nothing and says what is.
    let whole = a.dimension(
        Kind::LineRound {
            line: v,
            round: c2,
            far: true,
        },
        99.0,
        Point2::ZERO,
    );
    a.set_driven(whole, true);
    assert!(a.solve());
    assert_eq!(a.diagnose().dof, 0);
    assert!(a.diagnose().redundant.is_empty() && a.diagnose().conflicting.is_empty());
    let plane = workpiece_core::world::DatumPlane::new(
        workpiece_core::math::Point3::ZERO,
        workpiece_core::math::Point3::Z,
        workpiece_core::math::Point3::X,
    )
    .expect("a plane");
    let said = crate::annotate::annotations("s", a, &plane);
    let of = |id: Id| {
        &said
            .iter()
            .find(|(name, _)| *name == format!("s/d{id}"))
            .unwrap()
            .1
    };
    assert_eq!(of(whole).text(), "(13)");
    // A point 4 off the u axis, dimensioned about it: Ø8.
    let p = a.add_point(Point2::new(3.0, 3.0));
    let across = a.dimension_of(
        Kind::PointLine { point: p, line: u },
        8.0,
        Point2::ZERO,
        true,
    );
    assert!(a.solve());
    assert!((a.at(p).y - 4.0).abs() < 1e-9);
    let said = crate::annotate::annotations("s", a, &plane);
    let text = said
        .iter()
        .find(|(name, _)| *name == format!("s/d{across}"))
        .unwrap()
        .1
        .text();
    assert_eq!(text, "⌀8");
}

#[test]
fn what_is_drawn_elsewhere_snaps_as_a_sketch_does() {
    use crate::tools::Snappable;
    use workpiece_core::handles::CanvasView;
    let p = Point2::new;
    // A cross of two lines and a circle beside it; a pixel is 0.1.
    let snappable = Snappable::new(
        &[(p(0.0, 0.0), p(10.0, 0.0)), (p(5.0, -5.0), p(5.0, 5.0))],
        &[(p(20.0, 0.0), 2.0)],
        &[],
    );
    let view = CanvasView {
        pixel: 0.1,
        ..CanvasView::default()
    };
    let snap = |at: Point2, level: Option<Point2>| {
        let s = snappable.snap(at, &view, level, &[]);
        (s.at, s.on)
    };
    // An end of a line; where the two cross; the other line's end.
    assert_eq!(snap(p(0.3, 0.2), None), (p(0.0, 0.0), true));
    assert_eq!(snap(p(5.3, 0.3), None), (p(5.0, 0.0), true));
    assert_eq!(snap(p(5.2, -4.8), None).0, p(5.0, -5.0));
    // The circle's middle, and onto it.
    assert_eq!(snap(p(20.3, 0.2), None), (p(20.0, 0.0), true));
    let (on, hit) = snap(p(22.3, 0.0), None);
    assert!(hit && (on.distance(p(20.0, 0.0)) - 2.0).abs() < 1e-9);
    // Off everything, level with the corner before.
    let (level, hit) = snap(p(30.0, 10.4), Some(p(12.0, 10.0)));
    assert!(!hit);
    assert_eq!(level, p(30.0, 10.0));
    // A point hovered is remembered: off everything, the next snaps
    // level with it (straight above the line's end, a V guide from it).
    let hovered = snappable.snap(p(10.2, 0.1), &view, None, &[]);
    assert_eq!((hovered.point, hovered.badge), (Some(p(10.0, 0.0)), "◎"));
    let mut remembered = Vec::new();
    hovered.remember(&mut remembered);
    let above = snappable.snap(p(10.3, 7.0), &view, None, &remembered);
    assert_eq!(above.at, p(10.0, 7.0));
    assert_eq!(above.guides, [(p(10.0, 0.0), false)]);
    // On the circle and level with the hovered point as well: where
    // the level crosses the circle, near the pointer.
    let crossed = snappable.snap(p(21.98, 0.32), &view, None, &[p(5.0, 1.0)]);
    assert!(crossed.on && crossed.badge == "on");
    assert!((crossed.at.y - 1.0).abs() < 1e-9);
    assert!((crossed.at.distance(p(20.0, 0.0)) - 2.0).abs() < 1e-9);
    assert_eq!(crossed.guides, [(p(5.0, 1.0), true)]);
}

#[test]
fn a_datum_plane_projects_as_where_it_crosses_the_sketch() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    use workpiece_core::world::topo::nurbs::Recognized;
    use workpiece_core::world::{Datum, DatumPlane};
    // The plane x = 3 seen from a sketch on the plane z = 0: the line
    // x = 3; one parallel to the sketch does not cross it.
    let mut world = workpiece_core::World::default();
    world.datums.insert(
        "upright".into(),
        Datum::Plane(DatumPlane::new(
            Point3::new(3.0, 5.0, 7.0),
            Point3::X,
            Point3::Y,
        )?),
    );
    world.datums.insert(
        "level".into(),
        Datum::Plane(DatumPlane::new(
            Point3::new(0.0, 0.0, 2.0),
            Point3::Z,
            Point3::X,
        )?),
    );
    let xy = DatumPlane::new(Point3::ZERO, Point3::Z, Point3::X)?;
    let found = crate::project::project(&world, &xy, &Source::Datum("upright".into()))?;
    let [one] = &found[..] else {
        panic!("one line");
    };
    let Recognized::Line(a, b) = one.shape else {
        panic!("a line");
    };
    assert!(
        (a.x - 3.0).abs() < 1e-9 && (b.x - 3.0).abs() < 1e-9,
        "{a} {b}"
    );
    assert!((a.y - b.y).abs() > 1.0);
    assert!(crate::project::project(&world, &xy, &Source::Datum("level".into())).is_err());
    Ok(())
}

/// A square of four lines, each with its own end points (no coincidences),
/// the last corner `gap` apart.
fn loose_square(gap: f64) -> SketchArgs {
    let mut a = Sketch::new("s").args;
    let corners = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
    for k in 0..4 {
        let (from, to) = (corners[k], corners[(k + 1) % 4]);
        let p = a.add_point(Point2::new(from.0, from.1));
        let q = a.add_point(Point2::new(to.0, to.1 + if k == 3 { gap } else { 0.0 }));
        a.add_line(p, q);
    }
    a
}

#[test]
fn ends_at_one_place_close_an_area_and_a_gap_does_not() {
    let plane = workpiece_core::world::DatumPlane::new(
        workpiece_core::math::Point3::ZERO,
        workpiece_core::math::Point3::Z,
        workpiece_core::math::Point3::X,
    )
    .expect("a plane");
    let areas = |a: &SketchArgs| crate::profile::profile(a, plane).regions.len();
    // Loose ends at the same place, and a hair apart, are corners.
    assert_eq!(areas(&loose_square(0.0)), 1);
    assert_eq!(areas(&loose_square(1e-9)), 1);
    assert!(crate::profile::open_ends(&loose_square(0.0)).is_empty());
    // A real gap is open, and says where.
    let open = loose_square(0.1);
    assert_eq!(areas(&open), 0);
    assert_eq!(crate::profile::open_ends(&open).len(), 2);
    let note = crate::profile::profile(&open, plane).open_note();
    assert!(
        note.contains("open where") && note.contains("(0.0000, 0.1000)"),
        "{note}"
    );
}

#[test]
fn the_canvas_draws_a_dimension_as_the_shared_layout_has_it() {
    // One layout serves the canvas and the 3D view: whatever the layout
    // gives for a dimension is on the canvas, whichever way it is seen.
    let mut b = Bench::new();
    b.tool(Tool::Rectangle);
    b.click(0.0, 0.0);
    b.click(4.0, 3.0);
    let bottom = b.lines()[0];
    let (kind, value, _) = modify::dimension_for(
        &b.args,
        &[Item::Entity(bottom)],
        Point2::new(2.0, -1.0),
        &b.view,
    )
    .expect("a dimension");
    let id = b.args.dimension(kind, value, Point2::new(2.0, -1.0));
    for (right, up) in [
        (Point2::X, Point2::Y),
        (Point2::NEG_X, Point2::Y),
        (Point2::Y, Point2::NEG_X),
    ] {
        let view = CanvasView {
            right,
            up,
            ..view()
        };
        let all = crate::draw::shapes(&b.args, &b.state, &b.args.diagnose(), &view);
        let laid = crate::layout::dimension(
            &b.args,
            &b.args.constraints[&id],
            &view,
            workpiece_core::handles::Tone::Dimension,
        );
        assert!(
            laid.len() >= 4,
            "two extension lines, the line across, two arrows, the text"
        );
        for shape in &laid {
            assert!(all.contains(shape), "{shape:?} is not on the canvas");
        }
    }
}

#[test]
fn what_is_drawn_is_not_selected_and_the_dimension_tool_starts_from_nothing() {
    let mut b = Bench::new();
    b.tool(Tool::Rectangle);
    b.click(0.0, 0.0);
    b.click(4.0, 3.0);
    assert_eq!(b.lines().len(), 4);
    assert!(b.state.selection.is_empty());
    b.tool(Tool::Circle);
    b.click(8.0, 0.0);
    b.click(9.0, 0.0);
    assert!(b.state.selection.is_empty());
    // A spline, ended by a double click.
    b.tool(Tool::Spline);
    b.click(0.0, 6.0);
    b.click(2.0, 7.0);
    b.click(4.0, 6.0);
    b.event(CanvasEventKind::DoubleClick, 4.0, 6.0);
    assert!(b.state.selection.is_empty());
    // Something selected, then the dimension tool: nothing is.
    b.tool(Tool::Select);
    b.click(2.0, 0.0);
    assert_eq!(b.state.selection.len(), 1);
    b.tool(Tool::Dimension);
    assert!(b.state.selection.is_empty());
}

#[test]
fn a_rotation_axis_shows_before_its_second_click() {
    use workpiece_core::handles::{CanvasShape, Tone};
    let mut b = Bench::new();
    b.tool(Tool::RotationAxis);
    b.click(1.0, 1.0);
    b.hover(3.0, 4.0);
    // A centre line through the click towards the pointer, across the
    // view: far longer than from the click to the pointer.
    let shapes = canvas_of(&b).shapes(&b.view);
    let along = Point2::new(2.0, 3.0).normalize();
    let dashes: Vec<(Point2, Point2)> = shapes
        .iter()
        .filter_map(|s| match s {
            CanvasShape::Polyline {
                points,
                tone: Tone::Preview,
                ..
            } if points.len() == 2 => Some((points[0], points[1])),
            _ => None,
        })
        .filter(|(p, q)| {
            let on = |x: Point2| (x - Point2::new(1.0, 1.0)).perp_dot(along).abs() < 1e-9;
            on(*p) && on(*q)
        })
        .collect();
    let reach = dashes
        .iter()
        .flat_map(|(p, q)| [*p, *q])
        .map(|p| (p - Point2::new(1.0, 1.0)).dot(along))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), d| {
            (lo.min(d), hi.max(d))
        });
    assert!(dashes.len() > 10 && reach.1 - reach.0 > 20.0, "{reach:?}");
}

#[test]
fn a_point_on_a_line_is_level_with_the_one_before_as_well() {
    // A slanted line; a line drawn from (0, 2) to near where the level
    // y = 2 crosses it: on it, and level.
    let mut b = Bench::new();
    b.tool(Tool::Line);
    b.click(3.0, -1.0);
    b.click(7.0, 7.0);
    b.tool(Tool::Line);
    let slanted = b.lines()[0];
    b.click(0.0, 2.0);
    // The slanted line is at y = 2 where x = 4.5: hovered a little off.
    b.click(4.53, 2.04);
    let drawn = *b.lines().iter().find(|&&l| l != slanted).expect("drawn");
    let end = b.args.points_of(drawn)[1];
    assert!(b.args.at(end).abs_diff_eq(Point2::new(4.5, 2.0), 1e-9));
    let kinds: Vec<&Kind> = b.args.constraints.values().map(|c| &c.kind).collect();
    assert!(
        kinds.iter().any(
            |k| matches!(k, Kind::PointOn { point, entity } if *point == end && *entity == slanted)
        ),
        "{kinds:?}"
    );
    assert!(
        kinds
            .iter()
            .any(|k| matches!(k, Kind::Along { b: e, axis: SketchAxis::U, .. } if *e == end)),
        "{kinds:?}"
    );
}

#[test]
fn trimming_at_a_middle_point_lets_go_of_the_middle() {
    // A line from (0, 0) to (4, 0), and one from its middle up: a T.
    let mut b = Bench::new();
    b.tool(Tool::Line);
    b.click(0.0, 0.0);
    b.click(4.0, 0.0);
    b.tool(Tool::Line);
    let long = b.lines()[0];
    b.click(2.0, 0.02);
    b.click(2.0, 3.0);
    let middle = |b: &Bench| {
        b.args
            .constraints
            .values()
            .filter(|c| matches!(c.kind, Kind::Midpoint { line, .. } if line == long))
            .count()
    };
    assert_eq!(middle(&b), 1);
    let up = *b.lines().iter().find(|&&l| l != long).expect("the upright");
    let foot = b.args.points_of(up)[0];
    // The left half trimmed: the line now ends at the upright's foot,
    // the same point (not one only "on" the upright), and nothing holds
    // that foot at the middle any more.
    b.tool(Tool::Trim);
    b.click(1.0, 0.0);
    assert_eq!(middle(&b), 0);
    assert!(b.args.points_of(long).contains(&foot));
    let on_upright = b
        .args
        .constraints
        .values()
        .any(|c| matches!(c.kind, Kind::PointOn { entity, .. } if entity == up));
    assert!(!on_upright);
    let (a, c) = b.args.line(long).expect("the rest");
    assert!(
        a.abs_diff_eq(Point2::new(2.0, 0.0), 1e-9) && c.abs_diff_eq(Point2::new(4.0, 0.0), 1e-9)
    );
}
