//! The step being edited: its dialog and handles, producing [`Message`]s,
//! and finding things in them.

use shapecraft_cad_part::AnyOperation;
use shapecraft_core::handles::{Canvas, Handle, Space};
use shapecraft_core::world::DatumPlane;
use shapecraft_core::{EditContext, EditVisualization, Widget};

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
}

/// The step `model` is editing, if any.
pub(crate) fn session(model: &Model) -> Option<Session> {
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
    Some(Session { op, visualization })
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
    selection: &[shapecraft_core::Source],
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

impl shapecraft_core::Parts for Placeable<'_> {
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
