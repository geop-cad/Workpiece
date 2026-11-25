use shapecraft_cad_part::AnyOperation;
use shapecraft_core::widgets::Button;
use shapecraft_core::{Hotkey, Key, Placement};
use shapecraft_ops_datum::{AddAxis, AddFrame, AddPlane};

use super::Icon;
use crate::{Message, Model};

/// One entry of a menu: what it does, and whether it can now.
pub struct MenuItem {
    /// What it shows.
    pub icon: Icon,
    /// Whether it does anything now.
    pub enabled: bool,
    /// The button: its label and message.
    pub button: Button<Message>,
}

/// A datum the user can add: its operation, built from the selection,
/// and whether the selection exactly fits one of its constructions.
pub struct DatumOperation {
    /// What it shows.
    pub icon: Icon,
    /// Its name.
    pub label: &'static str,
    /// Adds it to the focused part and opens it.
    pub message: Message,
    /// Whether the selection is exactly what one of its modes takes.
    pub fits: bool,
}

/// The datums the user can add to `model`'s focused part — frame, plane
/// and axis — each built from the selection (its references the
/// construction takes, in the first mode they fit).
pub fn datum_operations(model: &Model) -> Vec<DatumOperation> {
    let Some(part) = model.focused_part() else {
        return Vec::new();
    };
    let Ok(program) = model.workspace.part(&part) else {
        return Vec::new();
    };
    let world = &model.viewport.shown.world;
    let selection = model.selected_sources();
    let op = |icon, label, op: AnyOperation, fits| DatumOperation {
        icon,
        label,
        message: Message::AddOperation {
            part: part.clone(),
            op,
        },
        fits,
    };
    // Exactly: every selected reference used, and the datum constructed.
    let plane = AddPlane::from_selection(program.fresh_id("plane"), world, &selection);
    let axis = AddAxis::from_selection(program.fresh_id("axis"), world, &selection);
    let frame = AddFrame::from_selection(program.fresh_id("frame"), world, &selection);
    let fits = [
        plane.fits(world, &selection),
        axis.fits(world, &selection),
        frame.fits(world, &selection),
    ];
    vec![
        op(Icon::Named("plane"), "Plane", plane.into(), fits[0]),
        op(Icon::Named("axis"), "Axis", axis.into(), fits[1]),
        op(Icon::Named("frame"), "Frame", frame.into(), fits[2]),
    ]
}

/// The menu at the pointer (the right or middle button, held): while a
/// step is edited, what the step offers for what is selected in it (a
/// sketch: the constraints and dimensions that fit) and closing its
/// dialog; otherwise every tool whose step takes what is selected, or
/// with nothing selected what one starts with.
pub struct ContextMenu {
    /// Where it opens, in viewport pixels.
    pub position: [f32; 2],
    /// Its items, in groups shown apart.
    pub groups: Vec<Vec<MenuItem>>,
}

impl ContextMenu {
    /// The context menu of `model`, while it is open.
    pub fn new(model: &Model) -> Option<Self> {
        let position = model.context_menu?;
        let item = |icon, label: &str, hotkey: Hotkey, message: Message| MenuItem {
            icon,
            enabled: true,
            button: Button::new(label, move || message.clone()).with_hotkey(hotkey),
        };
        let escape = Hotkey::named(Key::Escape);
        let groups = if let Some(session) = model.session() {
            // What the edited step offers for what is selected in it,
            // then closing its dialog.
            let offered: Vec<MenuItem> = session
                .visualization
                .menu
                .iter()
                .filter_map(|tool| {
                    let message = tool.click()?;
                    Some(item(
                        Icon::Named(tool.icon),
                        &tool.label,
                        tool.hotkey,
                        message,
                    ))
                })
                .collect();
            let ribbon = session.visualization.placement == Placement::Ribbon;
            let cancel = if ribbon {
                Hotkey::shift_named(Key::Escape)
            } else {
                escape
            };
            let mut groups = vec![
                offered,
                vec![
                    item(
                        Icon::Done,
                        "Done",
                        Hotkey::named(Key::Enter),
                        Message::FinishEdit,
                    ),
                    item(Icon::Cancel, "Cancel", cancel, Message::CancelEdit),
                ],
            ];
            groups.retain(|g| !g.is_empty());
            groups
        } else {
            // What can be made of what is selected: every tool whose
            // step takes all of it — or, with nothing selected, what
            // is built on first. The tools of the tab that shows come
            // first.
            let mut groups = vec![offered(model, &item)];
            if !model.selection.is_empty() {
                groups.push(vec![item(
                    Icon::Focus,
                    "Clear selection",
                    escape,
                    Message::ClearSelection,
                )]);
            }
            groups.retain(|g| !g.is_empty());
            groups
        };
        Some(Self { position, groups })
    }
}

/// How many tools the menu offers at most.
const OFFERED: usize = 9;

/// The tools the menu offers for what is selected in `model`: those
/// whose step takes all of it; with nothing selected, those that start
/// something (a sketch, the datums, a curve).
fn offered(model: &Model, item: &dyn Fn(Icon, &str, Hotkey, Message) -> MenuItem) -> Vec<MenuItem> {
    let Some(path) = model.focused_part() else {
        return Vec::new();
    };
    let Ok(part) = model.workspace.part(&path) else {
        return Vec::new();
    };
    let world = &model.viewport.shown.world;
    let selection = model.selected_sources();
    let fresh_id = |stem: &str| part.fresh_id(stem);
    let context = shapecraft_core::ToolContext {
        fresh_id: &fresh_id,
        world,
        unit: part.unit,
        selection: &selection,
    };
    let datums = datum_operations(model);
    let registry = shapecraft_cad_part::registry();
    let mut tools: Vec<&shapecraft_core::Tool> = registry.tools().iter().collect();
    // (Those of the tab that shows first; each once, though it may be
    // on several tabs.)
    tools.sort_by_key(|tool| tool.tab != model.tab.label());
    let mut seen: Vec<&str> = Vec::new();
    let mut items = Vec::new();
    for tool in tools {
        if seen.contains(&tool.label) || items.len() >= OFFERED {
            continue;
        }
        let made = (tool.make)(&context);
        let (op, all) = crate::edit::with_selection(model, &path, made, &selection);
        let fits = if selection.is_empty() {
            matches!(tool.group, "Sketch" | "Construct")
        } else if let Some(datum) = datums.iter().find(|d| d.label == tool.label) {
            // (A datum by its own constructions.)
            datum.fits
        } else {
            all
        };
        if !fits {
            continue;
        }
        seen.push(tool.label);
        let hotkey = if tool.key.is_ascii_lowercase() {
            Hotkey::shift(tool.key.to_ascii_uppercase())
        } else {
            Hotkey::key(tool.key)
        };
        let message = Message::AddOperation {
            part: path.clone(),
            op,
        };
        items.push(item(Icon::Named(tool.icon), tool.label, hotkey, message));
    }
    items
}
