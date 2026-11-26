//! The [`View`]: what is shown for a [`Model`], one module per area of
//! the window.

mod edit_dialog;
mod icon;
pub(crate) mod inspect;
mod menu;
mod overview;
mod picture;
mod properties;
mod ribbon;
mod sheet;
mod spread;
mod table;
mod timeline;
mod viewport;

pub use edit_dialog::EditDialog;
pub use icon::Icon;
pub use inspect::Reading;
pub use menu::{ContextMenu, DatumOperation, MenuItem, datum_operations};
pub use overview::{Item, Overview};
pub use picture::{Picture, camera, camera_of, lighting, scene};
pub use properties::{Materials, NewDrawing, PartProperties, SheetTemplates};
pub use ribbon::{Ribbon, TabButton, Tool};
pub use sheet::{
    Hovering, Mark, PickMode, SheetPick, Sheeted, candidates as sheet_candidates,
    describe as describe_sheet_pick, detail_handles, marks as sheet_marks, start_of,
};
pub use table::Table;
pub use timeline::{ProgramStep, REMOVE as REMOVE_STEP, Scrubber, Timeline, describe_source};
pub use viewport::{
    Entry, NoteShown, NoteText, Viewport, clip_polygon, describe_candidate, project,
};

use workpiece_core::{Hotkey, Placement, Widget};

use crate::{Message, Model};

/// What the application shows, derived from a [`Model`]. Its widgets
/// produce [`Message`](crate::Message)s for
/// [`Model::update`](crate::Model::update); afterwards the view is derived
/// again.
pub struct View {
    /// The tools.
    pub ribbon: Ribbon,
    /// The active part's steps.
    pub timeline: Timeline,
    /// What the focused part is made of.
    pub overview: Overview,
    /// The active part's path, and whether it differs from its file.
    pub document: Option<(String, bool)>,
    /// The popup editing a step, while one is edited.
    pub edit_dialog: Option<EditDialog>,
    /// The menu at the pointer, while it is open.
    pub context_menu: Option<ContextMenu>,
    /// The active part's parameters and variants.
    pub table: Option<Table>,
    /// Whether the table shows, in a panel over the view.
    pub table_shows: bool,
    /// The materials of the library, while their panel shows.
    pub materials: Option<Materials>,
    /// What the part says of itself, while that panel shows.
    pub properties: Option<PartProperties>,
    /// The sheet templates, while their panel shows.
    pub templates: Option<SheetTemplates>,
    /// What a new drawing can start from, while that is asked.
    pub new_drawing: Option<NewDrawing>,
    /// The picture that shows in place of the model: the step that is
    /// edited, if it is one.
    pub picture: Option<Picture>,
    /// The drawing that shows in place of the model: the step that is
    /// edited, if it is one.
    pub sheet: Option<SheetShown>,
    /// The 3D view.
    pub viewport: Viewport,
}

/// The drawing that is edited, as it shows.
pub struct SheetShown {
    /// The part the drawing is a step of.
    pub part: String,
    /// The step: what the drawing is called.
    pub id: String,
    /// The sheet.
    pub sheet: workpiece_ops_drawing::Sheet,
    /// Everything drawn of it.
    pub laid: std::sync::Arc<workpiece_ops_drawing::lay::Laid>,
    /// Whether what is missing on it shows as what it would be, and in
    /// which view (see [`Message::PreviewNotes`]).
    pub previews: Option<Option<String>>,
    /// The section views whose cutting line can be dragged on the
    /// sheet: those of a section along a sketch that only holds the
    /// line.
    pub movable: Vec<String>,
    /// The camera views the part has: what a view can be of.
    pub views: Vec<String>,
    /// The sections the part has: what a section view can be of.
    pub sections: Vec<String>,
    /// The properties parts have: what the cells of the title block
    /// that say one are of.
    pub schema: Vec<workpiece_part::Property>,
    /// The sheet templates: what the sheet can be made like.
    pub templates: Vec<workpiece_ops_drawing::Template>,
    /// What draws on it by hand: the canvas of a sketch lying on the
    /// sheet, in millimetres from its lower left corner. (Its tools are
    /// the edit dialog's.)
    pub hand: Option<Hand>,
}

/// The canvas that draws on a sheet by hand.
pub struct Hand {
    /// What it draws, for how the sheet is seen.
    pub draw: workpiece_core::handles::CanvasDraw,
    /// The message for an event of the pointer on the sheet.
    pub on_event: workpiece_core::handles::CanvasHandler<Message>,
    /// The text field it has open, if one is: a dimension's value.
    pub input: Option<workpiece_core::handles::CanvasInput<Message>>,
}

impl SheetShown {
    fn new(model: &Model) -> Option<Self> {
        let (part, drawing) = model.drawing()?;
        let sheet = drawing.args;
        let previews = model.previews.clone();
        let laid = model.with_subject(|subject| {
            workpiece_ops_drawing::lay::lay_out(
                &sheet,
                subject,
                &model.sheets_kept,
                previews.as_ref().map(|view| view.as_deref()),
            )
        })?;
        let hand = model.session().and_then(|session| {
            let (_, canvas) = session.canvas()?;
            Some(Hand {
                draw: canvas.draw.clone(),
                on_event: canvas.on_event.clone(),
                input: canvas.input.clone(),
            })
        });
        let movable = sheet
            .views
            .iter()
            .filter(|v| match &v.kind {
                workpiece_ops_drawing::sheet::ViewKind::Section {
                    of: Some(_),
                    section,
                    ..
                } => model.cut_sketch(section).is_some(),
                _ => false,
            })
            .map(|v| v.id.clone())
            .collect();
        let world = &model.viewport.shown.world;
        Some(Self {
            movable,
            views: world.views.keys().cloned().collect(),
            sections: world.sections.keys().cloned().collect(),
            schema: model.workspace.schema.properties.clone(),
            templates: model.workspace.sheets.templates.clone(),
            part,
            id: drawing.id,
            sheet,
            laid: std::sync::Arc::new(laid),
            previews,
            hand,
        })
    }

    /// The message that makes the drawing's sheet `sheet`.
    pub fn set(&self, sheet: workpiece_ops_drawing::Sheet) -> Message {
        let op = workpiece_ops_drawing::Drawing {
            id: self.id.clone(),
            args: sheet,
        };
        Message::ReplaceOperation {
            part: self.part.clone(),
            op: op.into(),
        }
    }
}

/// What a hotkey presses.
#[derive(Debug, Clone, PartialEq)]
pub enum Press {
    /// A button of the view, by its message.
    Message(Box<Message>),
    /// A tool of the edit dialog, by its widget path: its message is made
    /// when it is pressed.
    DialogTool(Vec<usize>),
    /// A tool or a check mark of the edit dialog's panel at the side (its
    /// inspector), by its widget path.
    InspectorTool(Vec<usize>),
    /// An option of a choice of the edit dialog: the choice's widget
    /// path, and the option.
    DialogChoice(Vec<usize>, usize),
    /// A tool of the edit dialog's context menu, by its place in it.
    MenuTool(usize),
}

impl View {
    /// The view of `model`.
    pub fn new(model: &Model) -> Self {
        Self {
            ribbon: Ribbon::new(model),
            timeline: Timeline::new(model),
            overview: Overview::new(model),
            document: model.active_part.clone().map(|p| {
                let dirty = model.is_dirty(&p);
                (p, dirty)
            }),
            edit_dialog: EditDialog::new(model),
            context_menu: ContextMenu::new(model),
            table: Table::new(model),
            table_shows: model.panel == Some(crate::Panel::Parameters),
            materials: Materials::new(model),
            properties: PartProperties::new(model),
            templates: SheetTemplates::new(model),
            new_drawing: NewDrawing::new(model),
            picture: Picture::new(model),
            sheet: SheetShown::new(model),
            viewport: Viewport::new(model),
        }
    }

    /// Every hotkey that does something now, with what it presses: the
    /// edited step's tools and buttons first (they win where keys are
    /// used twice), then the ribbon's tools (while they show), then the
    /// viewport's buttons.
    pub fn hotkeys(&self) -> Vec<(Hotkey, Press)> {
        let mut keys: Vec<(Hotkey, Press)> = Vec::new();
        let mut ribbon_shows = true;
        if let Some(dialog) = &self.edit_dialog {
            ribbon_shows = dialog.placement == Placement::Popup;
            // (What is selected first: its properties win over the
            // ribbon's tools of the same keys.)
            if let Some(inspector) = dialog.inspector() {
                let mut found = Vec::new();
                tools(inspector, &mut Vec::new(), &mut found);
                keys.extend(found.into_iter().map(|(key, press)| match press {
                    Press::DialogTool(path) => (key, Press::InspectorTool(path)),
                    other => (key, other),
                }));
            }
            let mut path = Vec::new();
            tools(dialog.fields(), &mut path, &mut keys);
            // (Then what the context menu offers now, by its keys: what
            // picks more or less, which needs no button of its own.)
            for (k, tool) in dialog.menu().iter().enumerate() {
                if tool.enabled() {
                    keys.push((tool.hotkey, Press::MenuTool(k)));
                }
            }
            for button in [&dialog.finish, &dialog.cancel, &dialog.toggle_preview] {
                keys.extend(
                    button
                        .hotkey
                        .map(|k| (k, Press::Message(Box::new(button.click())))),
                );
            }
        }
        // (A menu's tools by their own keys.)
        let offered = self.ribbon.tools.iter().flat_map(|t| &t.menu);
        for tool in self.ribbon.tools.iter().chain(offered) {
            // Undo and redo always show.
            let history = matches!(tool.icon, Icon::Undo | Icon::Redo);
            if tool.enabled && (ribbon_shows || history) {
                keys.extend(
                    tool.button
                        .hotkey
                        .map(|k| (k, Press::Message(Box::new(tool.button.click())))),
                );
            }
        }
        // The tabs, while the ribbon's own tools show.
        if ribbon_shows {
            keys.extend(self.ribbon.tabs.iter().filter_map(|tab| {
                let press = Press::Message(Box::new(tab.button.click()));
                Some((tab.button.hotkey?, press))
            }));
        }
        keys.extend(self.timeline.remove.iter().map(|path| {
            let remove = Message::RemoveOperation(path.clone());
            (timeline::REMOVE, Press::Message(Box::new(remove)))
        }));
        let viewport = [&self.viewport.home, &self.viewport.projection];
        for button in viewport.into_iter().chain(&self.viewport.normal) {
            keys.extend(
                button
                    .hotkey
                    .map(|k| (k, Press::Message(Box::new(button.click())))),
            );
        }
        // The first use of a key wins.
        let mut seen = Vec::new();
        keys.retain(|(key, _)| {
            let new = !seen.contains(key);
            seen.push(*key);
            new
        });
        keys
    }

    /// The message of what a hotkey presses.
    pub fn press(&self, press: &Press) -> Option<Message> {
        match press {
            Press::Message(message) => Some((**message).clone()),
            Press::DialogTool(path) => match self.edit_dialog.as_ref()?.fields().at(path)? {
                Widget::ToolButton(tool) => tool.click(),
                Widget::Button(button) => Some(button.click()),
                _ => None,
            },
            Press::MenuTool(k) => self.edit_dialog.as_ref()?.menu().get(*k)?.click(),
            Press::DialogChoice(path, option) => {
                match self.edit_dialog.as_ref()?.fields().at(path)? {
                    Widget::Choice(choice) => choice.set(*option),
                    _ => None,
                }
            }
            Press::InspectorTool(path) => {
                match self.edit_dialog.as_ref()?.inspector()?.at(path)? {
                    Widget::ToolButton(tool) => tool.click(),
                    Widget::Check(check) => check.toggle(),
                    _ => None,
                }
            }
        }
    }
}

/// The hotkeys of the tools and buttons in `widget` (at `path`) that can
/// be pressed now.
fn tools(widget: &Widget<Message>, path: &mut Vec<usize>, keys: &mut Vec<(Hotkey, Press)>) {
    match widget {
        Widget::ToolButton(tool) if tool.enabled() => {
            keys.push((tool.hotkey, Press::DialogTool(path.clone())));
        }
        Widget::Button(button) => {
            keys.extend(button.hotkey.map(|k| (k, Press::DialogTool(path.clone()))));
        }
        Widget::Check(check) if check.disabled.is_none() => {
            keys.extend(check.hotkey.map(|k| (k, Press::DialogTool(path.clone()))));
        }
        Widget::Choice(choice) => {
            for (option, key) in choice.hotkeys.iter().enumerate() {
                if let Some(key) = key {
                    keys.push((*key, Press::DialogChoice(path.clone(), option)));
                }
            }
        }
        _ => {}
    }
    for (i, child) in widget.children().iter().enumerate() {
        path.push(i);
        tools(child, path, keys);
        path.pop();
    }
}
