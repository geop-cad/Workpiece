//! The sketch's ribbon, its panel and its context menu: its tools, the
//! constraints and changes that fit what is selected, and the properties
//! of what is selected (a dimension's value, whether it is driven or a
//! diameter; whether curves are construction geometry).

use crate::{Id, Shape};
use std::rc::Rc;
use std::sync::Arc;

use workpiece_core::hotkey::{Hotkey, Key};
use workpiece_core::math::Point2;
use workpiece_core::widgets::{
    Check, Group, HBox, Label, Mood, NumericInput, ReferenceInput, ReferenceList, TextInput,
    ToolButton, VBox,
};
use workpiece_core::world::DatumPlane;
use workpiece_core::{Edit, Measure, Source, Widget, World};

use crate::model::{Item, Kind, SketchArgs};
use crate::modify::{self, Action};
use crate::project;
use crate::solve::Diagnosis;
use crate::tools::{self, SketchState, Tool};

type SketchEdit = Edit<SketchArgs, SketchState>;

/// What the ribbon is built from.
pub(crate) struct Session {
    pub(crate) args: Rc<SketchArgs>,
    pub(crate) state: Rc<SketchState>,
    pub(crate) world: Arc<World>,
    pub(crate) parameters: workpiece_core::Parameters,
    pub(crate) plane: Option<DatumPlane>,
    pub(crate) diagnosis: Rc<Diagnosis>,
}

/// A tool's button: its icon, name, hotkey, and what to do with it.
fn tool_info(tool: Tool) -> (&'static str, &'static str, Hotkey, &'static str) {
    match tool {
        Tool::Select => (
            "select",
            "Select",
            Hotkey::named(Key::Escape),
            "Click to select (Ctrl adds), drag points and curves, drag over nothing to select what is inside",
        ),
        Tool::Line => (
            "line",
            "Line",
            Hotkey::key('L'),
            "Click point after point. Hover back over the last point to switch to a tangent arc, and again for a line. Double-click to end",
        ),
        Tool::Rectangle => (
            "rectangle",
            "Rectangle",
            Hotkey::key('R'),
            "Click two opposite corners",
        ),
        Tool::CenterRectangle => (
            "center-rectangle",
            "Center rect",
            Hotkey::shift('R'),
            "Click the center, then a corner",
        ),
        Tool::Circle => (
            "circle",
            "Circle",
            Hotkey::key('C'),
            "Click the center, then a point on the circle",
        ),
        Tool::Circle2 => (
            "circle-2",
            "2-pt circle",
            Hotkey::shift('2'),
            "Click the two ends of a diameter",
        ),
        Tool::Circle3 => (
            "circle-3",
            "3-pt circle",
            Hotkey::shift('3'),
            "Click three points the circle goes through",
        ),
        Tool::Ellipse => (
            "ellipse",
            "Ellipse",
            Hotkey::shift('I'),
            "Click the center, the end of one axis, then how far the other reaches",
        ),
        Tool::Arc => (
            "arc",
            "Arc",
            Hotkey::key('A'),
            "Click the two ends, then a point on the arc",
        ),
        Tool::CenterArc => (
            "center-arc",
            "Center arc",
            Hotkey::shift('A'),
            "Click the center, the start, then the end",
        ),
        Tool::Slot => (
            "slot",
            "Slot",
            Hotkey::key('U'),
            "Click the centers of the two ends, then the width",
        ),
        Tool::Polygon => (
            "polygon",
            "Polygon",
            Hotkey::key('G'),
            "Click the center, then a corner",
        ),
        Tool::Spline => (
            "spline",
            "Spline",
            Hotkey::key('S'),
            "Click the control points; double-click to end",
        ),
        Tool::Point => (
            "point",
            "Point",
            Hotkey::key('N'),
            "Click where the point goes",
        ),
        Tool::Text => (
            "sketch-text",
            "Text",
            Hotkey::shift('K'),
            "Click where the text starts: the outlines of its letters, curves of the sketch held where they are written. What it says and how high its capitals are show beside the tools while it is in hand",
        ),
        Tool::RotationAxis => (
            "rotation-axis",
            "Rotation axis",
            Hotkey::shift('Z'),
            "The axis the sketch is turned about: click a point it passes through, then one for its direction. A centre line without end; a dimension from a point or a line to it, with the text put across it, is a diameter",
        ),
        Tool::Dimension => (
            "dimension",
            "Dimension",
            Hotkey::key('D'),
            "Click what to measure, then where the text goes, and type the value. Double-click a dimension to change it",
        ),
        Tool::Trim => (
            "trim",
            "Trim",
            Hotkey::key('W'),
            "Click a piece of a curve, or drag across pieces, to cut them away",
        ),
    }
}

/// An action's button: its icon, name, hotkey, and what it does.
fn action_info(action: Action) -> (&'static str, &'static str, Hotkey, &'static str) {
    match action {
        Action::Coincident => (
            "coincident",
            "Coincident",
            Hotkey::key('I'),
            "Points coincide, a point lies on a curve, circles share a center",
        ),
        Action::Horizontal => (
            "horizontal",
            "Horizontal",
            Hotkey::key('H'),
            "Level, as the view shows it",
        ),
        Action::Vertical => (
            "vertical",
            "Vertical",
            Hotkey::key('V'),
            "Upright, as the view shows it",
        ),
        Action::Parallel => (
            "parallel",
            "Parallel",
            Hotkey::key('B'),
            "Two lines are parallel",
        ),
        Action::Perpendicular => (
            "perpendicular",
            "Perpendicular",
            Hotkey::shift('L'),
            "Two lines are at a right angle",
        ),
        Action::Collinear => (
            "collinear",
            "Collinear",
            Hotkey::shift('C'),
            "Two lines lie on one line",
        ),
        Action::Tangent => (
            "tangent",
            "Tangent",
            Hotkey::key('T'),
            "A line and a circle, or two circles, touch",
        ),
        Action::Curvature => (
            "curvature",
            "Curvature",
            Hotkey::shift('U'),
            "A spline runs on from the line, arc or spline it ends on with the same direction and curvature (G2)",
        ),
        Action::Equal => (
            "equal",
            "Equal",
            Hotkey::key('E'),
            "Lines are as long, circles as large",
        ),
        Action::Midpoint => (
            "midpoint",
            "Midpoint",
            Hotkey::key('M'),
            "A point is the middle of a line",
        ),
        Action::Symmetric => (
            "symmetric",
            "Symmetric",
            Hotkey::key('Y'),
            "Two points mirror each other in a line",
        ),
        Action::Fix => (
            "fix",
            "Fix",
            Hotkey::key('F'),
            "Points stay where they are (again: let them go)",
        ),
        Action::Construction => (
            "construction",
            "Construction",
            Hotkey::key('X'),
            "Construction geometry is not part of the profile (again: make it part)",
        ),
        Action::Delete => (
            "delete",
            "Delete",
            Hotkey::named(Key::Delete),
            "Remove what is selected",
        ),
        Action::Mirror => (
            "mirror",
            "Mirror",
            Hotkey::shift('M'),
            "Copy the selected curves to the other side of the line selected last",
        ),
        Action::Offset => (
            "offset",
            "Offset",
            Hotkey::shift('O'),
            "Copy the selected curves a distance to their side",
        ),
        Action::Fillet => (
            "sketch-fillet",
            "Fillet",
            Hotkey::shift('F'),
            "Round the corner where two lines meet (select the corner, or the two lines); dimension the arc for its radius",
        ),
        Action::Chamfer => (
            "sketch-chamfer",
            "Chamfer",
            Hotkey::shift('H'),
            "Cut the corner where two lines meet off (select the corner, or the two lines)",
        ),
        Action::Split => (
            "sketch-split",
            "Split",
            Hotkey::shift('S'),
            "Split a line or an arc at a point (select both)",
        ),
        Action::Rotate => (
            "sketch-rotate",
            "Rotate",
            Hotkey::shift('T'),
            "Turn the selected curves about the selected point, by the Angle",
        ),
        Action::Scale => (
            "sketch-scale",
            "Scale",
            Hotkey::shift('X'),
            "Make the selected curves larger or smaller from the selected point, by the Factor",
        ),
        Action::Detach => (
            "sketch-detach",
            "Convert",
            Hotkey::shift('V'),
            "Make the selected projected curves curves of the sketch: free to change, no longer following the model",
        ),
        Action::Copy => (
            "sketch-copy",
            "Copy",
            Hotkey::shift('D'),
            "Copy the selected curves a little to the side: drag the copies where they go",
        ),
        Action::LinearPattern => (
            "sketch-linear",
            "Linear pattern",
            Hotkey::shift('P'),
            "Repeat the selected curves along the line selected last, a copy every length of it; Count says how many there are",
        ),
        Action::CircularPattern => (
            "sketch-circular",
            "Circular pattern",
            Hotkey::shift('N'),
            "Repeat the selected curves around the selected point; Count says how many there are",
        ),
        Action::Extend => (
            "sketch-extend",
            "Extend",
            Hotkey::shift('E'),
            "Run a line on until it meets a line or circle (select the line, then what it runs on to)",
        ),
    }
}

const DRAW: [Tool; 15] = [
    Tool::Line,
    Tool::RotationAxis,
    Tool::Rectangle,
    Tool::CenterRectangle,
    Tool::Circle,
    Tool::Circle2,
    Tool::Circle3,
    Tool::Ellipse,
    Tool::Arc,
    Tool::CenterArc,
    Tool::Slot,
    Tool::Polygon,
    Tool::Spline,
    Tool::Point,
    Tool::Text,
];

const CONSTRAIN: [Action; 11] = [
    Action::Coincident,
    Action::Horizontal,
    Action::Vertical,
    Action::Parallel,
    Action::Perpendicular,
    Action::Collinear,
    Action::Tangent,
    Action::Curvature,
    Action::Equal,
    Action::Midpoint,
    Action::Symmetric,
];

const MODIFY: [Action; 13] = [
    Action::Delete,
    Action::Rotate,
    Action::Scale,
    Action::Detach,
    Action::Mirror,
    Action::Offset,
    Action::Copy,
    Action::LinearPattern,
    Action::CircularPattern,
    Action::Fillet,
    Action::Chamfer,
    Action::Split,
    Action::Extend,
];

/// What the context menu offers besides: the properties of what is
/// selected, switched in one click.
const PROPERTIES: [Action; 2] = [Action::Construction, Action::Fix];

impl Session {
    fn tool(&self, tool: Tool) -> ToolButton<SketchEdit> {
        let (icon, label, hotkey, hint) = tool_info(tool);
        let (args, state) = (self.args.clone(), self.state.clone());
        ToolButton::new(icon, label, hotkey, move || {
            tools::switch(&args, &state, tool)
        })
        .with_hint(hint)
        .active(self.state.tool == tool)
    }

    fn action(&self, action: Action) -> ToolButton<SketchEdit> {
        let (icon, label, hotkey, hint) = action_info(action);
        let (args, state) = (self.args.clone(), self.state.clone());
        let label = match action {
            // Two circles: the same button, by its other name.
            Action::Coincident if modify::picked(&args, &state.selection).rounds.len() == 2 => {
                "Concentric"
            }
            _ => label,
        };
        let disabled = modify::unavailable(&self.args, &self.state, action);
        ToolButton::new(icon, label, hotkey, move || apply(&args, &state, action))
            .with_hint(hint)
            .disabled(disabled)
    }

    /// Whether what the drawing tools make is construction geometry: a
    /// mode of the tools, not a property of what is drawn already.
    fn construction_mode(&self) -> ToolButton<SketchEdit> {
        let state = self.state.clone();
        ToolButton::new("construction", "Construction", Hotkey::key('X'), move || {
            let mut next = (*state).clone();
            next.construction = !next.construction;
            Edit::state(next)
        })
        .with_hint(
            "What the drawing tools make from now on is construction geometry (again: part of the profile). Whether curves drawn already are is in their panel",
        )
        .active(self.state.construction)
    }

    /// The dimension button: with a selection that can be measured, it
    /// dimensions that right away; else it is the dimension tool, whose
    /// kind of dimension follows from what is picked and where its text
    /// is put.
    fn dimension(&self) -> ToolButton<SketchEdit> {
        let (args, state) = (self.args.clone(), self.state.clone());
        let fits = modify::no_dimension(&self.args, &self.state).is_none();
        let button = self.tool(Tool::Dimension).large();
        if !fits || self.state.tool == Tool::Dimension {
            return button;
        }
        ToolButton::new(button.icon, "Dimension", button.hotkey, move || {
            // The text goes above the middle of what is measured.
            let mut sum = Point2::ZERO;
            let mut count = 0.0;
            for item in &state.selection {
                let at = match *item {
                    Item::Point(p) => args.at(p),
                    Item::Entity(e) => args.middle(e),
                    Item::Constraint(_) => continue,
                };
                sum += at;
                count += 1.0;
            }
            let label = sum / f64::max(count, 1.0) + state.view.up * 40.0 * state.view.pixel;
            let mut next = (*state).clone();
            let Some((kind, value, diameter)) =
                modify::dimension_for(&args, &state.selection, label, &state.view)
            else {
                return Edit::state(next);
            };
            let (new, id, notice) = modify::place_dimension(&args, kind, value, label, diameter);
            next.selection = vec![Item::Constraint(id)];
            next.focus = notice.is_none().then_some(id);
            next.notice = notice;
            Edit::both(new, next)
        })
        .with_hint("Dimension what is selected")
        .large()
    }

    /// The panel of the one dimension selected: its value (a number, or
    /// a formula over the part's parameters, with its tolerance), whether
    /// it is driven, whether it is a diameter, and its name.
    fn dimension_panel(&self, id: Id) -> Option<Vec<Widget<SketchEdit>>> {
        let constraint = self.args.constraints.get(&id)?;
        let d = constraint.dimension.clone()?;
        let kind = constraint.kind;
        let mut fields: Vec<Widget<SketchEdit>> = vec![
            Label::new(
                "",
                format!("Dimension: {}", kind_name(&kind, &d)),
                Mood::Plain,
            )
            .into(),
        ];
        let (args, state) = (self.args.clone(), self.state.clone());
        if d.driven {
            // (A driven one only measures: what it is, in brackets.)
            let shown = crate::draw::dimension_text(constraint).0;
            fields.push(Label::new("Measures", shown, Mood::Plain).into());
        } else {
            let measure = Measure {
                value: d.value,
                formula: d.formula.clone(),
                tolerance: d.tolerance.clone(),
            };
            let parameters = self.parameters.clone();
            let angle = matches!(kind, Kind::Angle(..));
            let (args, state) = (args.clone(), state.clone());
            let mut value = NumericInput::measure(
                "Value",
                &measure,
                &self.parameters,
                move |entered: Measure| {
                    let mut next = (*state).clone();
                    next.focus = None;
                    match set_measure(&args, id, &entered, &parameters, angle) {
                        Ok(new) => {
                            next.notice = None;
                            Edit::both(new, next)
                        }
                        Err(why) => {
                            next.notice = Some(why);
                            Edit::state(next)
                        }
                    }
                },
            );
            if self.state.focus == Some(id) {
                value = value.focused(format!("dimension {id}"));
            }
            fields.push(value.into());
        }
        let driven = Check::new("Driven", d.driven, {
            let (args, state) = (args.clone(), state.clone());
            move |driven| {
                let mut next = (*state).clone();
                match modify::set_driven(&args, id, driven) {
                    Ok(new) => {
                        next.notice = None;
                        Edit::both(new, next)
                    }
                    Err(why) => {
                        next.notice = Some(why);
                        Edit::state(next)
                    }
                }
            }
        })
        .with_hint(
            "A driven dimension (in brackets) only measures what the rest of the sketch leaves it to be; a driving one holds its value. Driving is refused where it would over-define the sketch",
        );
        fields.push(driven.into());
        // A diameter where that applies: of a circle or an arc, or to an
        // axis.
        let to_axis = match kind {
            Kind::PointLine { line, .. } => self.args.is_endless(line),
            Kind::Size(_) => true,
            _ => false,
        };
        if SketchArgs::diameter_applies(&kind) && (to_axis || d.diameter) {
            let args = args.clone();
            let diameter = Check::new("Diameter", d.diameter, move |on| {
                let mut new = (*args).clone();
                new.set_diameter(id, on);
                Edit::args(new)
            })
            .with_hint(
                "Whether it gives the diameter (Ø): of the circle or arc, or about the axis as what the point sweeps when the sketch is turned about it",
            );
            fields.push(diameter.into());
        }
        let name = TextInput::new("Name", d.name.clone().unwrap_or_default(), {
            let args = args.clone();
            move |name: String| {
                let mut new = (*args).clone();
                let name = name.trim();
                if let Some(d) = new.dimension_mut(id) {
                    d.name = (!name.is_empty()).then(|| name.to_owned());
                }
                Edit::args(new)
            }
        });
        fields.push(name.into());
        Some(fields)
    }

    /// The panel of the points and curves selected: their properties
    /// (construction geometry, fixed, a spline's degree), and what the
    /// changes that repeat or turn them go by.
    fn selection_panel(&self) -> Option<Vec<Widget<SketchEdit>>> {
        let entities: Vec<Id> = self
            .state
            .selection
            .iter()
            .filter_map(|item| match item {
                Item::Entity(id) if self.args.entities.contains_key(id) => Some(*id),
                _ => None,
            })
            .collect();
        let points = self
            .state
            .selection
            .iter()
            .filter(|item| matches!(item, Item::Point(p) if self.args.points.contains_key(p)))
            .count();
        if entities.is_empty() && points == 0 {
            return None;
        }
        let title = match (&entities[..], points) {
            ([one], 0) => entity_name(&self.args, *one).to_owned(),
            ([], 1) => "Point".to_owned(),
            _ => format!("{} selected", entities.len() + points),
        };
        let mut fields: Vec<Widget<SketchEdit>> = vec![Label::new("", title, Mood::Plain).into()];
        let property = |action: Action, label: &str, on: bool, hotkey: Hotkey, hint: &str| {
            let (args, state) = (self.args.clone(), self.state.clone());
            modify::unavailable(&self.args, &self.state, action)
                .is_none()
                .then(|| {
                    Widget::from(
                        Check::new(label, on, move |_| apply(&args, &state, action))
                            .with_hotkey(hotkey)
                            .with_hint(hint),
                    )
                })
        };
        let construction = entities
            .iter()
            .all(|e| self.args.entities.get(e).is_some_and(|e| e.construction));
        fields.extend(property(
            Action::Construction,
            "Construction",
            construction,
            Hotkey::key('X'),
            "Construction geometry is there to constrain against, not part of the profile",
        ));
        let mut chosen: Vec<Id> = self
            .state
            .selection
            .iter()
            .filter_map(|item| match item {
                Item::Point(p) => Some(*p),
                _ => None,
            })
            .collect();
        for &e in &entities {
            chosen.extend(self.args.points_of(e));
        }
        let fixed = chosen
            .iter()
            .all(|p| self.args.points.get(p).is_some_and(|p| p.fixed));
        fields.extend(property(
            Action::Fix,
            "Fixed",
            fixed,
            Hotkey::key('F'),
            "Fixed points stay where they are",
        ));
        // A spline's degree, of the splines drawn here.
        let splines: Vec<Id> = entities
            .iter()
            .copied()
            .filter(|id| {
                self.args.entities.get(id).is_some_and(|e| {
                    e.projected.is_none() && matches!(e.shape, Shape::Spline { .. })
                })
            })
            .collect();
        if let Some(first) = splines.first() {
            let shown = match &self.args.entities[first].shape {
                Shape::Spline {
                    degree, control, ..
                } => (*degree).min(control.len().saturating_sub(1)).max(1),
                _ => 3,
            };
            let args = self.args.clone();
            let degree = NumericInput::new("Degree", shown as f64, move |n: f64| {
                let mut new = (*args).clone();
                for id in &splines {
                    if let Some(Shape::Spline {
                        degree,
                        control,
                        knots,
                        ..
                    }) = new.entities.get_mut(id).map(|e| &mut e.shape)
                    {
                        // (As high as its points allow; its knots evenly
                        // spread again.)
                        *degree =
                            (n.round() as usize).clamp(1, control.len().saturating_sub(1).max(1));
                        knots.clear();
                    }
                }
                new.solve();
                Edit::args(new)
            });
            fields.push(degree.into());
        }
        // What the patterns, Rotate and Scale go by.
        let state = self.state.clone();
        let count = NumericInput::new("Count", self.state.copies as f64, move |n: f64| {
            let mut next = (*state).clone();
            next.copies = (n.round() as usize).clamp(2, 200);
            Edit::state(next)
        });
        let state = self.state.clone();
        let angle = NumericInput::new("Angle °", self.state.angle, move |n: f64| {
            let mut next = (*state).clone();
            next.angle = n;
            Edit::state(next)
        });
        let state = self.state.clone();
        let factor = NumericInput::new("Factor", self.state.factor, move |n: f64| {
            let mut next = (*state).clone();
            if n > 0.0 {
                next.factor = n;
            }
            Edit::state(next)
        });
        fields.extend([count.into(), angle.into(), factor.into()]);
        Some(fields)
    }

    /// The panel at the side: the properties of what is selected; `None`
    /// while nothing is.
    pub(crate) fn inspector(&self) -> Option<Widget<SketchEdit>> {
        let fields = match self.state.selection[..] {
            [Item::Constraint(id)] => self.dimension_panel(id)?,
            _ => self.selection_panel()?,
        };
        Some(VBox::new(fields).into())
    }

    /// What projects the model into the sketch: with curves the sketch
    /// offers unasked selected (what lies in its plane, where its plane
    /// cuts the bodies), those; else a box of what is picked in the
    /// model.
    fn projection(&self) -> Widget<SketchEdit> {
        let offered: Vec<Id> = self
            .state
            .selection
            .iter()
            .filter_map(|item| match item {
                Item::Entity(id) if self.args.entities.get(id).is_some_and(|e| e.auto) => Some(*id),
                _ => None,
            })
            .collect();
        if !offered.is_empty() {
            let args = self.args.clone();
            return ToolButton::new("project", "Project", Hotkey::shift('J'), move || {
                let mut new = (*args).clone();
                for id in &offered {
                    if let Some(e) = new.entities.get_mut(id) {
                        // Drawn as a reference from now on; where the
                        // plane cuts, one of the sketch's own that
                        // follows nothing.
                        e.auto = false;
                        e.section = false;
                    }
                }
                Edit::args(new)
            })
            .with_hint("Project the selected curves the sketch offers (what lies in its plane, where its plane cuts the bodies): drawn as references from now on")
            .large()
            .into();
        }
        let sources: Vec<Source> = self
            .args
            .entities
            .values()
            .filter(|e| !e.auto)
            .filter_map(|e| e.projected.clone())
            .collect();
        let (args, world, plane) = (self.args.clone(), self.world.clone(), self.plane);
        ReferenceList::new("Project", project::kinds(), sources, move |wanted| {
            let mut new = (*args).clone();
            // What is no longer wanted goes.
            let gone: Vec<Item> = new
                .entities
                .iter()
                .filter(|(_, e)| {
                    !e.auto && e.projected.as_ref().is_some_and(|s| !wanted.contains(s))
                })
                .map(|(&id, _)| Item::Entity(id))
                .collect();
            new.remove(&gone);
            if let Some(plane) = plane {
                for source in &wanted {
                    for found in project::project(&world, &plane, source).unwrap_or_default() {
                        let at = found.source.clone();
                        let have = new
                            .entities
                            .values()
                            .any(|e| e.projected.as_ref() == Some(&at));
                        if !have {
                            project::place(&mut new, found);
                        }
                        // (What was offered is projected now.)
                        for e in new.entities.values_mut() {
                            if e.projected.as_ref() == Some(&at) {
                                e.auto = false;
                            }
                        }
                    }
                }
            }
            new.solve();
            Edit::args(new)
        })
        .into()
    }

    /// How constrained the sketch is, in a line.
    fn status(&self) -> Label {
        let d = &self.diagnosis;
        let (text, mood) = if let Some(notice) = &self.state.notice {
            (notice.clone(), Mood::Warning)
        } else if self.plane.is_none() {
            ("Pick a plane to sketch on".to_owned(), Mood::Warning)
        } else if !d.conflicting.is_empty() {
            ("Constraints conflict".to_owned(), Mood::Error)
        } else if !d.solved {
            ("Cannot be solved".to_owned(), Mood::Error)
        } else if !d.redundant.is_empty() {
            ("Redundant constraints".to_owned(), Mood::Warning)
        } else if d.dof == 0 {
            ("Fully constrained".to_owned(), Mood::Good)
        } else {
            (format!("{} degrees of freedom", d.dof), Mood::Plain)
        };
        Label::new("", text, mood)
    }

    /// The ribbon: the plane and status, then the tools in groups.
    pub(crate) fn ribbon(&self) -> Widget<SketchEdit> {
        let (args, world) = (self.args.clone(), self.world.clone());
        let plane = ReferenceInput::new("Plane", self.args.plane.clone(), move |plane| {
            let mut new = (*args).clone();
            // What lies in the plane is projected into the sketch.
            if let Ok(plane) = world.resolve(&plane) {
                project::auto(&mut new, &world, &plane);
            }
            new.plane = Some(plane);
            Edit::args(new)
        });
        let mut draw: Vec<Widget<SketchEdit>> = vec![self.tool(Tool::Select).into()];
        draw.extend(DRAW.map(|t| self.tool(t).into()));
        draw.push(self.construction_mode().into());
        // What the tool in hand makes: a polygon's sides, a spline's
        // degree and whether it goes through its clicks.
        if self.state.tool == Tool::Polygon {
            let state = self.state.clone();
            let sides = NumericInput::new("Sides", self.state.sides as f64, move |n: f64| {
                let mut next = (*state).clone();
                next.sides = (n.round() as usize).clamp(3, 64);
                Edit::state(next)
            });
            draw.push(sides.into());
        }
        if self.state.tool == Tool::Spline {
            let state = self.state.clone();
            let degree = NumericInput::new("Degree", self.state.degree as f64, move |n: f64| {
                let mut next = (*state).clone();
                next.degree = (n.round() as usize).clamp(1, 9);
                Edit::state(next)
            });
            draw.push(degree.into());
            let state = self.state.clone();
            let through = self.state.through;
            let toggle = ToolButton::new(
                "spline-through",
                if through {
                    "Through points"
                } else {
                    "Control points"
                },
                Hotkey::shift('W'),
                move || {
                    let mut next = (*state).clone();
                    next.through = !next.through;
                    Edit::state(next)
                },
            )
            .with_hint(
                "Whether the clicks are points the spline goes through, or its control points",
            );
            draw.push(toggle.into());
        }
        if self.state.tool == Tool::Text {
            let state = self.state.clone();
            let text = TextInput::new("Text", self.state.text.clone(), move |text| {
                let mut next = (*state).clone();
                next.text = text;
                Edit::state(next)
            });
            draw.push(text.into());
            let state = self.state.clone();
            let height = NumericInput::new("Height", self.state.height, move |n: f64| {
                let mut next = (*state).clone();
                if n > 0.0 {
                    next.height = n;
                }
                Edit::state(next)
            });
            draw.push(height.into());
        }
        let mut modify: Vec<Widget<SketchEdit>> = vec![self.tool(Tool::Trim).into()];
        modify.extend(MODIFY.map(|a| self.action(a).into()));
        // The grid: shown, and what clicks free of anything land on.
        let state = self.state.clone();
        let grid = ToolButton::new("grid", "Grid", Hotkey::shift('G'), move || {
            let mut next = (*state).clone();
            next.grid = !next.grid;
            Edit::state(next)
        })
        .with_hint(
            "A grid to draw on: what a drawing tool clicks lands on it, unless it snaps to something else",
        )
        .active(self.state.grid)
        .large();
        let head: Vec<Widget<SketchEdit>> = vec![plane.into(), self.status().into(), grid.into()];
        HBox::new(vec![
            Group::new("Sketch", head).into(),
            Group::new("Draw", draw).into(),
            Group::new("Project", vec![self.projection()]).into(),
            Group::new("Constrain", CONSTRAIN.map(|a| self.action(a).into()).into()).into(),
            Group::new("Dimension", vec![self.dimension().into()]).into(),
            Group::new("Modify", modify).into(),
        ])
        .into()
    }

    /// The context menu: what fits the selection; with nothing selected,
    /// the tools used most.
    pub(crate) fn menu(&self) -> Vec<ToolButton<SketchEdit>> {
        if self.state.selection.is_empty() {
            return [
                Tool::Select,
                Tool::Line,
                Tool::Rectangle,
                Tool::Circle,
                Tool::Dimension,
                Tool::Trim,
            ]
            .map(|t| self.tool(t))
            .into();
        }
        let mut menu: Vec<ToolButton<SketchEdit>> = CONSTRAIN
            .iter()
            .chain(&MODIFY)
            .chain(&PROPERTIES)
            .map(|&a| self.action(a))
            .filter(ToolButton::enabled)
            .collect();
        if modify::no_dimension(&self.args, &self.state).is_none() {
            menu.insert(0, self.dimension());
        }
        menu
    }
}

/// The sketch after `action` on what `state` selects, or why not (where
/// the user reads the status).
fn apply(args: &SketchArgs, state: &SketchState, action: Action) -> SketchEdit {
    let mut next = state.clone();
    match modify::apply(args, state, action) {
        Ok((new, selection)) => {
            next.selection = selection;
            Edit::both(new, next)
        }
        Err(why) => {
            next.notice = Some(why);
            Edit::state(next)
        }
    }
}

/// Dimension `id` of `args` given `entered`: its number or formula
/// (valued with `parameters`) and its tolerance. Refused, saying why,
/// for what is not positive, an angle of 180° or more, or a value that
/// over-defines the sketch (the old one stays).
fn set_measure(
    args: &SketchArgs,
    id: Id,
    entered: &Measure,
    parameters: &workpiece_core::Parameters,
    angle: bool,
) -> Result<SketchArgs, String> {
    let value = entered.resolve(parameters).map_err(|e| e.to_string())?;
    if value <= 0.0 {
        return Err("Dimensions are positive".to_owned());
    }
    if angle && value >= 180.0 {
        return Err("Angles are below 180°".to_owned());
    }
    let mut new = args.clone();
    if let Some(d) = new.dimension_mut(id) {
        d.formula = entered.formula.clone();
        d.tolerance = entered.tolerance.clone();
    }
    let before = args.diagnose().conflicting;
    if !new.set_value(id, value)
        || new
            .diagnose()
            .conflicting
            .iter()
            .any(|c| !before.contains(c))
    {
        return Err(
            "This value conflicts with the rest of the sketch: the old one stays".to_owned(),
        );
    }
    Ok(new)
}

/// What a dimension of `kind` is called, as `d` makes it.
fn kind_name(kind: &Kind, d: &crate::model::Dimension) -> &'static str {
    match kind {
        _ if d.diameter => "diameter",
        Kind::Size(_) => "radius",
        Kind::Angle(..) => "angle",
        Kind::Offset { .. } => "offset",
        _ => "distance",
    }
}

/// What entity `id` of `args` is called.
fn entity_name(args: &SketchArgs, id: Id) -> &'static str {
    match args.entities.get(&id).map(|e| &e.shape) {
        _ if args.is_axis(id) => "Axis of the plane",
        Some(Shape::Point(_)) => "Point",
        Some(Shape::Line(..)) => "Line",
        Some(Shape::Circle { .. }) => "Circle",
        Some(Shape::Arc { .. }) => "Arc",
        Some(Shape::Spline { .. }) => "Spline",
        Some(Shape::Axis { .. }) => "Rotation axis",
        None => "",
    }
}
