//! The step being edited: its dialog and handles, producing [`Message`]s,
//! and finding things in them.

use std::rc::Rc;

use workpiece_core::handles::{Canvas, Handle, Space};
use workpiece_core::world::DatumPlane;
use workpiece_core::{EditContext, EditVisualization, Widget};
use workpiece_part::AnyOperation;

use crate::Message;
use crate::Model;
use crate::model::focus::locate;

/// The step the model is editing.
pub(crate) struct Session {
    /// The step.
    pub(crate) op: AnyOperation,
    /// Its dialog and handles, in the focused part's coordinates; their
    /// messages replace the step.
    pub(crate) visualization: EditVisualization<Message>,
    /// The lines of what the step takes along with what is picked
    /// ([`EditVisualization::implied`]), in the world it is applied
    /// to, once they are drawn.
    pub(crate) implied: std::cell::OnceCell<Vec<Vec<workpiece_core::math::Point3>>>,
}

/// What a [`Session`] is made from: the model as far as the edited
/// step's dialog and handles can see it.
struct Inputs {
    workspace: workpiece_part::Workspace,
    active: Option<String>,
    editing: Vec<String>,
    variant: Option<String>,
    state: Option<workpiece_core::AnyEditState>,
    /// (Held, so that it is the same world while it is held.)
    before: std::sync::Arc<workpiece_core::World>,
    placed: std::collections::BTreeMap<(String, Option<String>), Vec<String>>,
    snaps: workpiece_core::handles::SnapKinds,
}

impl Inputs {
    fn of(model: &Model) -> Self {
        Self {
            workspace: model.workspace.clone(),
            active: model.active_part.clone(),
            editing: model.editing.clone(),
            variant: model.variant.clone(),
            state: model.edit_state.clone(),
            before: model.viewport.shown.before.clone(),
            placed: model.viewport.shown.placed.clone(),
            snaps: model.settings.snaps,
        }
    }

    /// Whether `model` is what these were taken from.
    fn are_of(&self, model: &Model) -> bool {
        std::sync::Arc::ptr_eq(&self.before, &model.viewport.shown.before)
            && self.editing == model.editing
            && self.active == model.active_part
            && self.variant == model.variant
            && self.state == model.edit_state
            && self.placed == model.viewport.shown.placed
            && self.snaps == model.settings.snaps
            && self.workspace == model.workspace
    }
}

thread_local! {
    /// The session made last on this thread, with what it was made
    /// from. A step's dialog and handles can take geometry to work out
    /// (where an arrow stands on the edge rounded, the middle of the
    /// face moved): they are worked out once for each state of the
    /// model, not again on every frame and every move of the pointer.
    static LAST: std::cell::RefCell<Option<(Inputs, Rc<Session>)>> =
        const { std::cell::RefCell::new(None) };
}

/// The step `model` is editing, if any.
pub(crate) fn session(model: &Model) -> Option<Rc<Session>> {
    if model.editing.is_empty() {
        // (What the last edit was made from is let go.)
        LAST.set(None);
        return None;
    }
    let kept = LAST.with_borrow(|last| {
        last.as_ref()
            .filter(|(inputs, _)| inputs.are_of(model))
            .map(|(_, session)| session.clone())
    });
    if let Some(session) = kept {
        return Some(session);
    }
    let session = Rc::new(make_session(model)?);
    LAST.set(Some((Inputs::of(model), session.clone())));
    Some(session)
}

fn make_session(model: &Model) -> Option<Session> {
    let (id, focus) = model.editing.split_last()?;
    let active = model.active_part.as_deref()?;
    let located = locate(&model.workspace, active, focus).ok()?;
    let path = located.part;
    let part = model.workspace.part(&path).ok()?;
    let op = part.operation(id)?.clone();
    // Formulas entered are valued as the part is shown.
    let variant = located.variant.or_else(|| model.variant.clone());
    let parameters = part.resolve(variant.as_deref()).unwrap_or_default();
    let context = EditContext {
        step: "",
        world: &model.viewport.shown.before,
        cache: &part.cache,
        parameters: &parameters,
        parts: &Placeable(model),
    };
    let step = id.clone();
    let visualization = op
        .visualize_edit(&context, model.edit_state.as_ref())
        .map(move |edit| {
            let replace = edit.op.map(|op| Message::ReplaceOperation {
                part: path.clone(),
                op,
            });
            let keep = edit.kept.map(|kept| Message::Keep {
                part: path.clone(),
                id: step.clone(),
                kept,
            });
            let state = edit.state.map(Message::EditState);
            let mut all: Vec<Message> = [replace, keep, state].into_iter().flatten().collect();
            if all.len() == 1 {
                all.remove(0)
            } else {
                Message::Batch(all)
            }
        });
    let visualization = with_snaps(visualization, model.settings.snaps);
    Some(Session {
        op,
        visualization,
        implied: std::cell::OnceCell::new(),
    })
}

/// `visualization`, for a step edited in the scene (by a space) in the
/// ribbon, with a group of toggles at its end: what a point moved or
/// placed snaps to, each kind by itself, and all at once.
fn with_snaps(
    mut visualization: EditVisualization<Message>,
    snaps: workpiece_core::handles::SnapKinds,
) -> EditVisualization<Message> {
    use workpiece_core::handles::SnapKind;
    use workpiece_core::widgets::{Group, HBox, ToolButton};
    let in_scene = visualization
        .handles
        .values()
        .any(|h| matches!(h, Handle::Space(_)));
    if !in_scene || visualization.placement != workpiece_core::Placement::Ribbon {
        return visualization;
    }
    let all = ToolButton::new(
        "snap",
        "Snap",
        workpiece_core::Hotkey::ctrl_shift('0'),
        move || {
            Message::SetSnaps(workpiece_core::handles::SnapKinds {
                on: !snaps.on,
                ..snaps
            })
        },
    )
    .with_hint("Snap what is moved or placed in the scene, to what the toggles beside say (hold Alt to move freely for a moment)")
    .active(snaps.on);
    let mut toggles: Vec<Widget<Message>> = vec![all.into()];
    for (k, kind) in SnapKind::ALL.into_iter().enumerate() {
        let on = snaps.has(kind);
        let key = char::from_digit(k as u32 + 1, 10).unwrap_or('9');
        let hint = match kind {
            SnapKind::Point => {
                "Snap to points: corners, the points of sketches, the origins of datums"
            }
            SnapKind::Middle => "Snap to the middles of edges",
            SnapKind::Centre => "Snap to the centres of circles and arcs, and of flat faces",
            SnapKind::Quad => "Snap to the quadrant points of circles",
            SnapKind::Crossing => "Snap to where two edges cross, as they are seen",
            SnapKind::Perpendicular => {
                "Snap to the foot of the perpendicular from where the drag started onto an edge"
            }
            SnapKind::Edge => "Snap onto edges, at the point nearest the pointer",
            SnapKind::Grid => {
                "Snap to the grid, whose spacing follows the zoom (along an arrow of the gizmo: to whole steps of that axis)"
            }
        };
        toggles.push(
            ToolButton::new(
                kind.icon(),
                kind.label(),
                workpiece_core::Hotkey::ctrl_shift(key),
                move || Message::SetSnaps(snaps.with(kind, !on)),
            )
            .with_hint(hint)
            .active(on && snaps.on)
            .into(),
        );
    }
    let group: Widget<Message> = Group::new("Snap", toggles).into();
    visualization.dialog = match visualization.dialog {
        Widget::HBox(mut row) => {
            row.children.push(group);
            Widget::HBox(row)
        }
        other => HBox::new(vec![other, group]).into(),
    };
    visualization
}

/// The plane the step `model` is editing is edited in, if it has one, in
/// the focused part's coordinates.
pub(crate) fn edit_plane(model: &Model) -> Option<DatumPlane> {
    let (id, focus) = model.editing.split_last()?;
    let active = model.active_part.as_deref()?;
    let path = locate(&model.workspace, active, focus).ok()?.part;
    let part = model.workspace.part(&path).ok()?;
    let parameters = part.resolve(None).unwrap_or_default();
    let context = EditContext {
        step: "",
        world: &model.viewport.shown.before,
        cache: &part.cache,
        parameters: &parameters,
        parts: &model.workspace,
    };
    part.operation(id)?.edit_plane(&context)
}

impl Session {
    /// The canvas among the step's handles, with its name, if it has one:
    /// it takes the pointer's events.
    pub(crate) fn canvas(&self) -> Option<(&str, &Canvas<Message>)> {
        self.visualization
            .handles
            .iter()
            .find_map(|(name, h)| match h {
                Handle::Canvas(canvas) => Some((name.as_str(), canvas)),
                _ => None,
            })
    }

    /// The space among the step's handles, with its name, if it has
    /// one: it takes the pointer's events as rays.
    pub(crate) fn space(&self) -> Option<(&str, &Space<Message>)> {
        self.visualization
            .handles
            .iter()
            .find_map(|(name, h)| match h {
                Handle::Space(space) => Some((name.as_str(), space)),
                _ => None,
            })
    }
}

/// `op`, a step about to be added to the part at `part` of `model`, with
/// what is selected put into its dialog's reference fields — each
/// source into the first field that takes it and has room — and
/// whether all that is selected found a place in it (or was in it
/// already, as in a datum made from the selection).
pub(crate) fn with_selection(
    model: &Model,
    part: &str,
    mut op: AnyOperation,
    selection: &[workpiece_core::Source],
) -> (AnyOperation, bool) {
    let Ok(program) = model.workspace.part(part) else {
        return (op, false);
    };
    let parameters = program
        .resolve(model.variant.as_deref())
        .unwrap_or_default();
    let world = &model.viewport.shown.world;
    let context = EditContext {
        step: "",
        world,
        cache: &program.cache,
        parameters: &parameters,
        parts: &model.workspace,
    };
    let mut all = true;
    for source in selection {
        if op.references().contains(source) {
            continue;
        }
        let dialog = op.visualize_edit(&context, None).dialog;
        let taken = find_map(&dialog, &|w| match w {
            Widget::ReferenceInput(input)
                if input.value.is_none() && input.accepts(world, source) =>
            {
                input.set(source.clone()).op
            }
            Widget::ReferenceList(list)
                if !list.values.contains(source) && list.accepts(world, source) =>
            {
                list.toggle(source.clone()).op
            }
            _ => None,
        });
        match taken {
            Some(with) => op = with,
            None => all = false,
        }
    }
    (op, all)
}

/// What `wanted` makes of the first widget in `root` it makes anything
/// of.
fn find_map<M: 'static, T>(
    root: &Widget<M>,
    wanted: &dyn Fn(&Widget<M>) -> Option<T>,
) -> Option<T> {
    wanted(root).or_else(|| {
        root.children()
            .iter()
            .find_map(|child| find_map(child, wanted))
    })
}

/// The widget at `path` (child indices from `root`).
pub(crate) fn widget_at<'a, M: 'static>(
    root: &'a Widget<M>,
    path: &[usize],
) -> Option<&'a Widget<M>> {
    root.at(path)
}

/// Where picking goes next in `root`: the first reference field with
/// nothing picked, else the first reference box (which keeps taking
/// references) while the chosen construction lacks references, else a
/// box that has to hold something and is empty, as a widget path.
pub(crate) fn first_empty_reference<M: 'static>(root: &Widget<M>) -> Option<Vec<usize>> {
    // A field that may stay empty is not asked for.
    let empty = |w: &Widget<M>| match w {
        Widget::ReferenceInput(input) => input.value.is_none() && input.optional.is_none(),
        // A joint's connectors, one after the other.
        Widget::ConnectorInput(input) => input.value.is_none(),
        // A mechanism's joints, one after the other.
        Widget::JointInput(input) => input.value.is_none(),
        _ => false,
    };
    // A construction whose chosen mode does not fit yet.
    let lacking = |w: &Widget<M>| matches!(w, Widget::Choice(c) if !c.enabled.get(c.selected).copied().unwrap_or(true));
    let list = |w: &Widget<M>| matches!(w, Widget::ReferenceList(_));
    // A box that has to hold something and holds nothing.
    let wanting =
        |w: &Widget<M>| matches!(w, Widget::ReferenceList(l) if l.required && l.values.is_empty());
    find(root, &empty)
        .or_else(|| find(root, &lacking).and_then(|_| find(root, &list)))
        .or_else(|| find(root, &wanting))
}

/// A reference field of a dialog: its path, its label, what is in it
/// (written out, to tell it from others), and whether that is nothing.
pub(crate) struct Field {
    path: Vec<usize>,
    label: String,
    held: String,
    empty: bool,
}

/// The reference fields of `root`, in order.
pub(crate) fn reference_fields<M: 'static>(root: &Widget<M>) -> Vec<Field> {
    fn walk<M: 'static>(w: &Widget<M>, path: &mut Vec<usize>, out: &mut Vec<Field>) {
        let field = match w {
            Widget::ReferenceInput(input) => Some((
                &input.label,
                format!("{:?}", input.value),
                input.value.is_none(),
            )),
            Widget::ReferenceList(list) => Some((
                &list.label,
                format!("{:?}", list.values),
                list.values.is_empty(),
            )),
            Widget::ConnectorInput(input) => Some((
                &input.label,
                format!("{:?}", input.value),
                input.value.is_none(),
            )),
            Widget::JointInput(input) => Some((
                &input.label,
                format!("{:?}", input.value),
                input.value.is_none(),
            )),
            _ => None,
        };
        if let Some((label, held, empty)) = field {
            out.push(Field {
                path: path.clone(),
                label: label.clone(),
                held,
                empty,
            });
        }
        for (i, child) in w.children().iter().enumerate() {
            path.push(i);
            walk(child, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(root, &mut Vec::new(), &mut out);
    out
}

/// Of the reference fields `after` the first that a change brought —
/// not among the fields `before` it, as they come in order (one only
/// filled or emptied in its place is the same field) — if nothing is in
/// it yet: the picker a mode chosen in the dialog brought up.
pub(crate) fn brought_reference(before: &[Field], after: &[Field]) -> Option<Vec<usize>> {
    let same = |a: &Field, b: &Field| a.label == b.label && a.held == b.held;
    let mut k = 0;
    for (j, field) in after.iter().enumerate() {
        let Some(was) = before.get(k) else {
            if field.empty {
                return Some(field.path.clone());
            }
            continue;
        };
        // The same, or the same field changed in its place (the one
        // after it is not the one that was here).
        let next_is_was = after.get(j + 1).is_some_and(|n| same(n, was));
        if same(field, was) || (field.label == was.label && !next_is_was) {
            k += 1;
            continue;
        }
        if field.empty {
            return Some(field.path.clone());
        }
    }
    None
}

/// The path of the field in `root` that picks a sketch, if it has one.
pub(crate) fn sketch_field<M: 'static>(root: &Widget<M>) -> Option<Vec<usize>> {
    find(
        root,
        &|w| matches!(w, Widget::ReferenceInput(input) if input.kind == workpiece_core::ReferenceKind::Profile),
    )
}

/// The path of the first widget in `root` that `wanted` takes.
fn find<M: 'static>(root: &Widget<M>, wanted: &dyn Fn(&Widget<M>) -> bool) -> Option<Vec<usize>> {
    if wanted(root) {
        return Some(Vec::new());
    }
    root.children().iter().enumerate().find_map(|(i, child)| {
        let mut path = find(child, wanted)?;
        path.insert(0, i);
        Some(path)
    })
}

/// The parts that can be placed, as the model knows them: the
/// workspace's, and the bodies of those the focused part places as its
/// last build found them.
struct Placeable<'a>(&'a Model);

impl workpiece_core::Parts for Placeable<'_> {
    fn variants(&self, path: &str) -> Vec<String> {
        self.0.workspace.variants(path)
    }

    fn materials(&self) -> Vec<String> {
        self.0.workspace.materials()
    }

    fn bodies(&self, path: &str, variant: Option<&str>) -> Vec<String> {
        let key = (path.to_owned(), variant.map(str::to_owned));
        let placed = &self.0.viewport.shown.placed;
        placed.get(&key).cloned().unwrap_or_default()
    }
}
