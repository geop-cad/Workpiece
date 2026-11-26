//! The [`Model`]: the entire application state, changed only by
//! [`Message`]s, with undo.
//!
//! Besides the workspace, it holds where the user is: the active part,
//! the placed part in focus, the step being edited, the scrubber, the
//! timeline's expanded nodes, and the [`Viewport`] with what is built and
//! what is under the pointer.

pub mod camera;
pub mod display;
pub mod focus;
mod history;
pub mod viewport;
mod views;

pub use camera::{Camera, Transition};
pub use viewport::Viewport;

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context;
use glam::{Vec2, Vec3};
use workpiece_core::ReferenceKind;
use workpiece_core::handles::{
    CanvasEvent, CanvasEventKind, Gizmo, GizmoPart, Handle, Snap, SnapKind, SnapKinds, SnapTargets,
};
use workpiece_core::math::Point3;
use workpiece_core::world::{DatumPlane, ORIGIN, Visibility};
use workpiece_core::{Pose, Ray, Source, Widget, World};
use workpiece_part::{AnyEditState, Part, Workspace};
use workpiece_rasterize::{Scene, handles};

use crate::Message;
use crate::edit::{self, Session};
use crate::message::{CameraMove, Pointer};
use crate::picking::{self, Candidate};
use focus::locate;
use history::{EditKey, History};
use viewport::PointerAt;

/// A build running on the engine's threads.
#[derive(Debug, Clone)]
struct Pending {
    /// What stops it.
    cancel: workpiece_part::Cancel,
    /// Where what it made arrives.
    done: std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Receiver<viewport::Shown>>>,
}

/// An edit that waits while a sketch is drawn for it.
#[derive(Debug, Clone)]
struct Returning {
    /// The step it is of.
    editing: Vec<String>,
    /// The undo depth when its dialog opened.
    checkpoint: Option<usize>,
    /// Whether the step was added by it.
    added: bool,
}

/// What the user prefers: kept by the host from one session to the next.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Whether the model shows cut at the plane of a sketch that is
    /// drawn on a plane, so what is in front of it hides nothing.
    pub sketch_section: bool,
    /// The part files opened or saved last in this workspace, most
    /// recent first (paths as the workspace has them), at most
    /// [`RECENT`].
    pub recent: Vec<String>,
    /// What a point moved or placed in the scene snaps to.
    pub snaps: workpiece_core::handles::SnapKinds,
}

/// How many part files [`Settings::recent`] remembers.
pub const RECENT: usize = 10;

impl Default for Settings {
    fn default() -> Self {
        Self {
            sketch_section: true,
            recent: Vec::new(),
            snaps: workpiece_core::handles::SnapKinds::default(),
        }
    }
}

impl Settings {
    /// Puts `path` first among the recent files.
    pub fn remember(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_owned());
        self.recent.truncate(RECENT);
    }
}

/// The entire application state.
#[derive(Debug, Clone, Default)]
pub struct Model {
    /// Every part.
    pub workspace: Workspace,
    /// The path of the part being worked on; `None` when none is.
    pub active_part: Option<String>,
    /// The step being edited, as a step path from the active part; empty
    /// when none is. Everything but its last id is the focus.
    pub editing: Vec<String>,
    /// The placed part in focus, as a step path of `AddPart` steps from
    /// the active part; empty for the active part itself.
    pub focus: Vec<String>,
    /// The step selected in the timeline, as a step path from the active
    /// part.
    pub step: Option<Vec<String>>,
    /// The tab of the ribbon that shows.
    pub tab: crate::message::Tab,
    /// How many steps of the focused part are shown; `None` for all.
    pub scrub: Option<usize>,
    /// The expanded tree nodes of the timeline: step paths of `AddPart`
    /// steps.
    pub expanded: BTreeSet<Vec<String>>,
    /// The variant the active part is shown as; `None` for the part as it
    /// is by default.
    pub variant: Option<String>,
    /// The state of the edit session, if the edited step has one: its
    /// tool in use, what is selected inside it. Neither saved nor undone.
    pub edit_state: Option<AnyEditState>,
    /// The edit dialog's reference field being picked, by widget path.
    pub picking: Option<Vec<usize>>,
    /// What the user selected while not editing: a set, in the order
    /// selected.
    pub selection: Vec<Candidate>,
    /// Where the context menu is open, in viewport pixels.
    pub context_menu: Option<[f32; 2]>,
    /// How the model is displayed: cut open, its parts apart.
    pub display: display::Display,
    /// What the user prefers about how the model is shown.
    pub settings: Settings,
    /// What the display was before a sketch was drawn on a plane, and
    /// the cut that came with the sketch (to tell whether the user
    /// changed it since): the display is put back when the sketch is
    /// left.
    sketch_section: Option<(display::Display, Option<workpiece_core::world::Cutting>)>,
    /// Datums, bodies and sketches the user hid, as (part path, name).
    pub user_hidden: BTreeSet<(String, String)>,
    /// Datums the user showed although later steps use them (which hides
    /// them by default), as (part path, name).
    pub user_shown: BTreeSet<(String, String)>,
    /// Whether edits skip the preview: the edited step is not applied
    /// (nor what follows it rebuilt) until the dialog closes. For large
    /// assemblies.
    pub preview_disabled: bool,
    /// Whether snapping is held off for now (while a key is held).
    pub snaps_held: bool,
    /// The size datums are drawn at, if not as large as the model makes
    /// them (see [`World::datum_size`]).
    pub datum_size: Option<f64>,
    /// Each part as last read from or written to its file, by path; a
    /// part not in here was never saved.
    pub saved: BTreeMap<String, Part>,
    /// What builds the parts, and keeps what it built.
    pub engine: workpiece_part::Engine,
    /// Whether parts are built on the engine's threads, beside the
    /// interface, if the engine has any: what they make shows when
    /// [`poll`](Self::poll) finds it done.
    pub background: bool,
    /// The build running beside the interface.
    pending: Option<Pending>,
    /// What is drawn of each world, kept while the world is the same.
    pub scenes: std::sync::Arc<workpiece_rasterize::Scenes>,
    /// The viewport.
    pub viewport: Viewport,
    /// Undo and redo for the workspace.
    history: History,
    /// The undo depth when the edit dialog opened, for cancelling.
    edit_checkpoint: Option<usize>,
    /// Whether the edited step was just added and not yet changed (no
    /// reference picked, no value entered): its errors wait till it is.
    edit_untouched: bool,
    /// Whether the edited step was started by its tool going on after
    /// the step before it was placed (a dimension after a dimension):
    /// left untouched, it goes when anything else is turned to.
    edit_again: bool,
    /// Whether something was made for the edited step on the way that
    /// stays if the edit is cancelled (a sketch drawn for it).
    edit_keeps: bool,
    /// Whether the edited step was added by this edit: cancelling takes
    /// it away again.
    edit_added: bool,
    /// The edit to go back to when this one ends: that of the step a
    /// sketch was made for, on a plane picked where it asked for a
    /// sketch.
    returning: Option<Returning>,
    /// What the views of sheets show, kept from one drawing of them to
    /// the next.
    pub sheets_kept: workpiece_ops_drawing::lay::Kept,
    /// Whether the drawing that is edited shows what is missing on it
    /// as what it would be, and in which view (see
    /// [`Message::PreviewNotes`]).
    pub previews: Option<Option<String>>,
    /// The picture that is edited, as the view last turned to where
    /// it was taken from.
    edit_shot: Option<String>,
    /// The panel that shows over the view, if one was asked for.
    pub panel: Option<crate::Panel>,
    /// What the picture that is edited is traced from, kept while the
    /// model and its materials are the same.
    pub(crate) picture_kept: PictureKept,
    /// What was last found again of what steps referred to (see
    /// [`Self::notice`]).
    notice: Option<String>,
    /// The plane the edited step is edited in, as the view last turned
    /// to face it.
    edit_plane: Option<DatumPlane>,
    /// The text of a sketch's dimension that is dragged, shown where the
    /// pointer has it (see [`HeldNote`]).
    pub(crate) held_note: Option<HeldNote>,
}

/// The text of a sketch's dimension that is being dragged: where it
/// stands is the sketch's, and moving it builds the sketch and every step
/// after it again, which takes long. So while it is dragged it only
/// shows where the pointer has it; once dropped, the sketch takes it and
/// is built again — once — and it shows there until that build does.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HeldNote {
    /// The annotation's name.
    pub(crate) name: String,
    /// Where its text stands, in the focused part's coordinates, in its
    /// sketch's plane.
    pub(crate) at: workpiece_core::math::Point3,
    /// Whether it was dropped, and the sketch has it.
    pub(crate) dropped: bool,
}

/// What a picture is traced from, kept by what tells it from another.
#[derive(Default)]
pub(crate) struct PictureKept(
    pub(crate) std::sync::Mutex<Option<(u64, std::sync::Arc<workpiece_render::Scene>)>>,
);

impl std::fmt::Debug for PictureKept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PictureKept")
    }
}

/// (Another model keeps its own.)
impl Clone for PictureKept {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Model {
    /// A model over `workspace` with no part being worked on.
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            ..Self::default()
        }
    }

    /// Applies `message`. Changes to the workspace are recorded for undo;
    /// one that fails leaves the model unchanged.
    pub fn update(&mut self, message: Message) -> anyhow::Result<()> {
        // A step its tool went on to by itself, with nothing done to
        // it: turning to anything else puts the tool down.
        if self.edit_again
            && self.edit_untouched
            && matches!(
                message,
                Message::AddOperation { .. }
                    | Message::Edit(_)
                    | Message::FinishEdit
                    | Message::SetTab(_)
            )
        {
            self.edit_again = false;
            self.update(Message::CancelEdit)?;
            if message == Message::FinishEdit {
                return Ok(());
            }
        }
        let rebuild = match message {
            Message::StopBuild => {
                if let Some(pending) = self.pending.take() {
                    pending.cancel.cancel();
                    self.viewport.shown.error = Some(
                        "The build was stopped: what is shown is the part as it was. \
                         Change or remove the step that takes long to build it again."
                            .to_owned(),
                    );
                }
                return Ok(());
            }
            Message::Open(path) => {
                self.workspace.part(&path)?;
                *self = Self {
                    workspace: std::mem::take(&mut self.workspace),
                    active_part: Some(path),
                    history: std::mem::take(&mut self.history),
                    user_hidden: std::mem::take(&mut self.user_hidden),
                    user_shown: std::mem::take(&mut self.user_shown),
                    saved: std::mem::take(&mut self.saved),
                    preview_disabled: self.preview_disabled,
                    settings: self.settings.clone(),
                    tab: self.tab,
                    // (What builds the parts, and where, stays: a part
                    // opened later builds beside the interface as the
                    // first did.)
                    engine: self.engine.clone(),
                    background: self.background,
                    datum_size: self.datum_size,
                    viewport: Viewport {
                        camera: self.viewport.camera,
                        size: self.viewport.size,
                        ..Viewport::default()
                    },
                    ..Self::default()
                };
                // A part read from its file is one opened recently.
                if let Some(path) = self.active_part.clone()
                    && self.saved.contains_key(&path)
                {
                    self.settings.remember(&path);
                }
                self.history.seal();
                true
            }
            Message::Load { parts, open } => {
                for (path, part) in parts {
                    if !self.workspace.parts.contains_key(&path) {
                        self.saved.insert(path.clone(), part.clone());
                        self.workspace.parts.insert(path, part);
                    }
                }
                if let Some(path) = open {
                    return self.update(Message::Open(path));
                }
                true
            }
            Message::New(path) => {
                anyhow::ensure!(
                    !self.workspace.parts.contains_key(&path),
                    "{path:?} is open already"
                );
                self.workspace.parts.insert(path.clone(), Part::default());
                return self.update(Message::Open(path));
            }
            Message::ReplacePart { part, placed } => {
                let path = self.active_part.clone().context("no active part")?;
                if !self.editing.is_empty() {
                    self.update(Message::CancelEdit)?;
                }
                self.change(None, |ws| {
                    for (at, placed) in placed {
                        anyhow::ensure!(at != path, "{path} cannot place itself");
                        ws.parts.insert(at, placed);
                    }
                    *ws.part_mut(&path)? = *part;
                    Ok(())
                })?;
                self.history.seal();
                true
            }
            Message::Close => {
                let Some(path) = self.active_part.take() else {
                    return Ok(());
                };
                let placed = self
                    .workspace
                    .parts
                    .values()
                    .any(|p| p.placed_parts().contains(&path));
                if !placed {
                    self.workspace.parts.remove(&path);
                    self.saved.remove(&path);
                }
                self.history = History::default();
                self.finish_edit();
                self.focus.clear();
                self.scrub = None;
                self.selection.clear();
                true
            }
            Message::Saved(path) => {
                let part = self.workspace.part(&path)?.clone();
                self.settings.remember(&path);
                self.saved.insert(path, part);
                false
            }
            Message::SaveAs(path) => {
                let old = self.active_part.clone().context("no active part")?;
                anyhow::ensure!(
                    path == old || !self.workspace.parts.contains_key(&path),
                    "{path:?} is open already"
                );
                let part = self.workspace.part(&old)?.clone();
                let placed = self
                    .workspace
                    .parts
                    .values()
                    .any(|p| p.placed_parts().contains(&old));
                if !placed {
                    self.workspace.parts.remove(&old);
                }
                self.workspace.parts.insert(path.clone(), part);
                self.active_part = Some(path);
                self.history = History::default();
                true
            }
            Message::SetVisible { name, visible } => {
                let part = self.focused_part().context("no active part")?;
                let key = (part, name);
                if visible {
                    self.user_hidden.remove(&key);
                    self.user_shown.insert(key);
                } else {
                    self.user_shown.remove(&key);
                    self.user_hidden.insert(key);
                }
                true
            }
            Message::PlaceNote { note, view, at } => {
                let (part, drawing) = self.drawing().context("no drawing is open")?;
                let mut sheet = drawing.args.clone();
                anyhow::ensure!(
                    !sheet.notes.iter().any(|n| n.note == note),
                    "{note} is on the sheet already"
                );
                let placed = self.with_subject(|subject| {
                    workpiece_ops_drawing::lay::place(
                        &sheet,
                        subject,
                        &self.sheets_kept,
                        &note,
                        view.as_deref(),
                    )
                });
                let (view, by_itself) = placed
                    .flatten()
                    .with_context(|| format!("no view of the sheet sees {note} true"))?;
                sheet.notes.push(workpiece_ops_drawing::sheet::PlacedNote {
                    note,
                    view,
                    at: at.unwrap_or(by_itself),
                    decimals: None,
                    starts: [None; 2],
                    count: None,
                    to: None,
                });
                let op = workpiece_ops_drawing::Drawing {
                    id: drawing.id,
                    args: sheet,
                };
                self.update(Message::ReplaceOperation {
                    part,
                    op: op.into(),
                })?;
                self.history.seal();
                false
            }
            Message::PlaceLine { view, of } => {
                let (part, drawing) = self.drawing().context("no drawing is open")?;
                let mut sheet = drawing.args.clone();
                anyhow::ensure!(
                    !sheet
                        .lines
                        .iter()
                        .any(|l| l.of == of && l.view.as_ref().is_none_or(|v| *v == view)),
                    "the line of {} is on the view {view} already",
                    of.label()
                );
                sheet.lines.push(workpiece_ops_drawing::sheet::SheetLine {
                    view: Some(view),
                    of,
                    ends: [0.0; 2],
                    hidden: false,
                    gaps: Vec::new(),
                    symmetry: false,
                    label: None,
                    label_at: None,
                });
                let op = workpiece_ops_drawing::Drawing {
                    id: drawing.id,
                    args: sheet,
                };
                self.update(Message::ReplaceOperation {
                    part,
                    op: op.into(),
                })?;
                self.history.seal();
                false
            }
            Message::PreviewNotes(previews) => {
                self.previews = previews;
                false
            }
            Message::MoveNote { name, at, settled } => {
                let part = self.focused_part().context("no active part")?;
                // (Kept where it is in the part itself, wherever the
                // part is placed.)
                let back = self.viewport.shown.pose.inverse();
                let at = back.transform_point(workpiece_core::math::Point3::from_array(at));
                let key = (part.clone(), format!("note {name}"));
                // A sketch's dimension stands where its sketch says: its
                // text is moved there, in the sketch's plane.
                let of = self
                    .viewport
                    .shown
                    .world
                    .annotations
                    .get(&name)
                    .and_then(|note| note.sketch.clone());
                if let Some(of) = of {
                    // (Where the line of sight through where it is
                    // dropped meets the sketch's plane.)
                    let camera = &self.viewport.camera;
                    let seen = self.viewport.shown.pose.transform_point(at);
                    let sight = if camera.orthographic {
                        camera.back().as_dvec3()
                    } else {
                        seen - camera.eye().as_dvec3()
                    };
                    let sight = back.transform_vector(sight);
                    let normal = of.x.cross(of.y);
                    let across = sight.dot(normal);
                    let at = if across.abs() > 1e-9 * sight.length() {
                        at - sight * ((at - of.origin).dot(normal) / across)
                    } else {
                        at
                    };
                    // While it is dragged it is only shown there: the
                    // sketch, and every step after it, is built again
                    // once, when it is dropped.
                    self.held_note = Some(HeldNote {
                        name,
                        at,
                        dropped: settled,
                    });
                    if !settled {
                        return Ok(());
                    }
                    let label = workpiece_core::math::Point2::new(
                        (at - of.origin).dot(of.x),
                        (at - of.origin).dot(of.y),
                    );
                    self.change(Some(key), |ws| {
                        let sketch = ws
                            .part_mut(&part)?
                            .operation_mut(&of.id)
                            .and_then(|op| op.downcast_mut::<workpiece_ops_sketch::Sketch>())
                            .with_context(|| format!("there is no sketch {:?}", of.id))?;
                        let dimension = sketch
                            .args
                            .constraints
                            .get_mut(&of.constraint)
                            .and_then(|c| c.dimension.as_mut())
                            .context("the sketch has no such dimension")?;
                        dimension.label = label;
                        Ok(())
                    })?;
                    self.history.seal();
                    // (The sketch is built again, its dimension there;
                    // till that shows, it is shown where it was dropped.)
                    true
                } else {
                    // (Kept as far from what it is about as it is put.)
                    let from = self
                        .viewport
                        .shown
                        .world
                        .annotations
                        .get(&name)
                        .map_or(workpiece_core::math::Point3::ZERO, |note| note.origin());
                    self.change(Some(key), |ws| {
                        let places = &mut ws.part_mut(&part)?.note_places;
                        places.insert(name, (at - from).to_array());
                        Ok(())
                    })?;
                    if settled {
                        self.history.seal();
                    }
                    false
                }
            }
            Message::SetPartMaterial(material) => {
                let part = self.focused_part().context("no active part")?;
                self.change(None, |ws| {
                    ws.part_mut(&part)?.material = material;
                    Ok(())
                })?;
                true
            }
            Message::SetLibrary { materials } => {
                self.change(None, |ws| {
                    ws.library.materials = materials;
                    Ok(())
                })?;
                true
            }
            Message::ShowPanel(panel) => {
                self.panel = panel;
                false
            }
            Message::TakeView => {
                let view = Some(self.stance());
                if let Some((part, mut seen)) = self.camera_view() {
                    seen.args.view = view;
                    return self.update(Message::ReplaceOperation {
                        part,
                        op: seen.into(),
                    });
                }
                let (part, mut render) = self.render().context("no picture is edited")?;
                render.args.view = view;
                return self.update(Message::ReplaceOperation {
                    part,
                    op: render.into(),
                });
            }
            Message::LookFrom(name) => {
                self.look_from(&name)?;
                false
            }
            Message::NewDrawing(template) => return self.new_drawing(template.as_deref()),
            Message::CutSection { view, line, at } => {
                self.cut_section(&view, &line, at)?;
                true
            }
            Message::FlipSection { view } => {
                self.flip_section(&view)?;
                true
            }
            Message::MoveSectionLine {
                view,
                line,
                settled,
            } => {
                self.move_section_line(&view, &line, settled)?;
                true
            }
            Message::SetProperty { name, value } => {
                let part = self.focused_part().context("no active part")?;
                if let Some(property) = self.workspace.schema.get(&name) {
                    property
                        .kind
                        .check(&value)
                        .map_err(|why| anyhow::anyhow!("{name}: {why}"))?;
                }
                let key = (part.clone(), format!("property {name}"));
                self.change(Some(key), |ws| {
                    let properties = &mut ws.part_mut(&part)?.properties;
                    match value.trim() {
                        "" => drop(properties.remove(&name)),
                        value => drop(properties.insert(name, value.to_owned())),
                    }
                    Ok(())
                })?;
                false
            }
            Message::SetSchema { properties } => {
                self.change(None, |ws| {
                    ws.schema.properties = properties;
                    Ok(())
                })?;
                false
            }
            Message::SetTemplates { templates } => {
                self.change(None, |ws| {
                    ws.sheets.templates = templates;
                    Ok(())
                })?;
                false
            }
            Message::SetUnit(unit) => {
                let part = self.focused_part().context("no active part")?;
                self.change(None, |ws| {
                    ws.part_mut(&part)?.unit = unit;
                    Ok(())
                })?;
                false
            }
            Message::SetDisplay(display) => {
                self.display = display;
                true
            }
            Message::Isolate(name) => {
                let part = self.focused_part().context("no active part")?;
                let topo = &self.viewport.shown.world.topo;
                let bodies: Vec<String> = topo
                    .bodies()
                    .ids()
                    .filter_map(|b| topo.name_of(b).map(str::to_owned))
                    .collect();
                let others = || bodies.iter().filter(|b| **b != name);
                let alone = others().all(|b| self.user_hidden.contains(&(part.clone(), b.clone())));
                for body in &bodies {
                    let key = (part.clone(), body.clone());
                    if *body == name || alone {
                        self.user_hidden.remove(&key);
                    } else {
                        self.user_shown.remove(&key);
                        self.user_hidden.insert(key);
                    }
                }
                true
            }
            Message::ContextMenu(at) => {
                self.context_menu = at;
                false
            }
            Message::ClearSelection => {
                self.selection.clear();
                false
            }
            Message::ViewportSize(size) => {
                self.viewport.size = size;
                false
            }
            Message::AddOperation { part, op } => {
                let depth = self.history.depth();
                let id = op.id().to_owned();
                // With the focused part scrubbed back, the step goes in at
                // the scrubber (and the scrubber after it); else at the end.
                let focused = self.focused_part().as_deref() == Some(&part);
                let steps = self.workspace.part(&part)?.program.len();
                let at = self.scrub.filter(|&s| focused && s < steps);
                self.change(None, |ws| {
                    let program = &mut ws.part_mut(&part)?.program;
                    anyhow::ensure!(
                        program.iter().all(|o| o.id() != op.id()),
                        "operation {:?} exists",
                        op.id()
                    );
                    program.insert(at.unwrap_or(program.len()), op);
                    Ok(())
                })?;
                if focused {
                    self.scrub = at.map(|k| k + 1);
                    self.editing = self.focus.iter().cloned().chain([id.clone()]).collect();
                    self.edit_checkpoint = Some(depth);
                    self.edit_untouched = true;
                    self.edit_added = true;
                    // The selection went into the step.
                    self.selection.clear();
                    self.pick_next();
                    self.take_last_sketch(&part, &id)?;
                }
                true
            }
            Message::ReplaceOperation { part, op } => {
                self.edit_untouched = false;
                let key = (part.clone(), op.id().to_owned());
                // The reference fields of the dialog before the change.
                let before = (self.editing.last() == Some(&key.1))
                    .then(|| self.session())
                    .flatten()
                    .map(|s| edit::reference_fields(&s.visualization.dialog));
                self.change(Some(key), |ws| {
                    let slot = ws
                        .part_mut(&part)?
                        .operation_mut(op.id())
                        .with_context(|| format!("no operation {:?}", op.id()))?;
                    *slot = op;
                    Ok(())
                })?;
                // A mode chosen that brings up a new picker: picking goes
                // to it at once, so the next click in the view fills it.
                if let Some(before) = before
                    && let Some(session) = self.session()
                {
                    let after = edit::reference_fields(&session.visualization.dialog);
                    if let Some(field) = edit::brought_reference(&before, &after) {
                        self.picking = Some(field);
                        self.viewport.candidates.clear();
                    }
                }
                // Without the preview, the shown world does not change.
                !self.preview_disabled || self.editing.is_empty()
            }
            // (Without a path it is the host's: it asks for one.)
            Message::NewPartHere(None) => false,
            Message::NewPartHere(Some(path)) => {
                let within = self.focused_part().context("no part to add it to")?;
                anyhow::ensure!(
                    !self.workspace.parts.contains_key(&path),
                    "there is a part {path:?} already"
                );
                self.finish_edit();
                let name = std::path::Path::new(&path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .filter(|s| !s.is_empty() && !s.contains('/'))
                    .unwrap_or_else(|| "part".to_owned());
                let mut id = name.clone();
                self.change(None, |ws| {
                    ws.parts.insert(path.clone(), Part::default());
                    let part = ws.part_mut(&within)?;
                    if part.operation(&id).is_some() {
                        id = part.fresh_id(&name);
                    }
                    part.program.push(
                        workpiece_ops_instances::AddPart {
                            id: id.clone(),
                            args: workpiece_ops_instances::AddPartArgs {
                                part: path.clone(),
                                variant: None,
                                hold: Default::default(),
                                bodies: Vec::new(),
                            },
                        }
                        .into(),
                    );
                    Ok(())
                })?;
                self.history.seal();
                // On to the new part, where it is in the assembly.
                let mut focus = self.focus.clone();
                focus.push(id);
                self.set_focus(focus);
                self.tab = crate::message::Tab::Modeling;
                true
            }
            Message::Keep { part, id, kept } => {
                self.edit_untouched = false;
                let key = (part.clone(), format!("{id}/kept"));
                self.change(Some(key), |ws| {
                    ws.part_mut(&part)?.cache.replace(&id, kept);
                    Ok(())
                })?;
                true
            }
            Message::MoveOperation { path, to } => {
                let (part, id) = self.step_at(&path)?;
                self.finish_edit();
                self.change(None, |ws| ws.part_mut(&part)?.move_operation(&id, to))?;
                self.history.seal();
                true
            }
            Message::RemoveOperation(path) => {
                let (part, id) = self.step_at(&path)?;
                self.finish_edit();
                self.change(None, |ws| ws.part_mut(&part)?.remove_operation(&id))?;
                self.history.seal();
                // What was selected may be gone with it.
                self.step = None;
                self.selection.clear();
                true
            }
            Message::RenameOperation { path, name } => {
                let (part, id) = self.step_at(&path)?;
                let name = name.trim().to_owned();
                self.finish_edit();
                self.change(None, |ws| ws.part_mut(&part)?.rename_operation(&id, &name))?;
                self.history.seal();
                // Whatever went by the old name goes by the new one.
                let renamed = |p: &mut Vec<String>| {
                    if p.starts_with(&path) {
                        p[path.len() - 1].clone_from(&name);
                    }
                };
                renamed(&mut self.focus);
                self.step.iter_mut().for_each(renamed);
                self.expanded = std::mem::take(&mut self.expanded)
                    .into_iter()
                    .map(|mut p| {
                        renamed(&mut p);
                        p
                    })
                    .collect();
                for names in [&mut self.user_hidden, &mut self.user_shown] {
                    *names = std::mem::take(names)
                        .into_iter()
                        .map(|(p, n)| {
                            let mut source = workpiece_core::Source::Datum(n);
                            if p == part {
                                source.rename(&id, &name);
                            }
                            (p, source.name().to_owned())
                        })
                        .collect();
                }
                self.selection.clear();
                true
            }
            Message::SetTab(tab) => {
                self.tab = tab;
                false
            }
            Message::SelectStep(path) => {
                self.step = path;
                false
            }
            Message::SetSketchSection(on) => {
                self.settings.sketch_section = on;
                false
            }
            Message::SetSnaps(snaps) => {
                self.settings.snaps = snaps;
                false
            }
            Message::SuspendSnaps(held) => {
                self.snaps_held = held;
                false
            }
            Message::SetPreview(on) => {
                self.preview_disabled = !on;
                true
            }
            Message::SetTable {
                part,
                parameters,
                variants,
            } => {
                self.change(None, |ws| {
                    let part = ws.part_mut(&part)?;
                    let mut names: Vec<&str> = parameters.iter().map(|p| p.name.as_str()).collect();
                    names.sort_unstable();
                    anyhow::ensure!(
                        names.windows(2).all(|w| w[0] != w[1]),
                        "two parameters have the same name"
                    );
                    part.parameters = parameters;
                    part.variants = variants;
                    Ok(())
                })?;
                // A variant that is gone no longer shows.
                let gone = self.variant.as_ref().is_some_and(|shown| {
                    !self
                        .active_part()
                        .is_some_and(|p| p.variants.iter().any(|v| v.name == *shown))
                });
                if gone {
                    self.variant = None;
                }
                true
            }
            Message::ShowVariant(variant) => {
                if let Some(name) = &variant {
                    let part = self.active_part().context("no active part")?;
                    anyhow::ensure!(
                        part.variants.iter().any(|v| v.name == *name),
                        "no variant {name:?}"
                    );
                }
                self.variant = variant;
                true
            }
            Message::EditState(state) => {
                self.edit_state = Some(state);
                false
            }
            Message::Batch(messages) => {
                for message in messages {
                    self.update(message)?;
                }
                false
            }
            Message::Tick(seconds) => {
                if let Some(transition) = &mut self.viewport.transition {
                    transition.progress += seconds / camera::TRANSITION_SECONDS;
                    self.viewport.camera = transition.camera();
                    if transition.progress >= 1.0 {
                        self.viewport.transition = None;
                    }
                }
                false
            }
            Message::Undo => self.history.undo(&mut self.workspace),
            Message::Redo => self.history.redo(&mut self.workspace),
            Message::EndEdit => {
                self.history.seal();
                false
            }
            Message::Edit(path) => {
                let (id, focus) = path.split_last().context("no step to edit")?;
                let active = self.active_part.as_deref().context("no active part")?;
                let part = locate(&self.workspace, active, focus)?.part;
                self.workspace
                    .part(&part)?
                    .operation(id)
                    .with_context(|| format!("no step {id:?} in part {part:?}"))?;
                self.finish_edit();
                // Within the part in focus, the scrubber stays where it
                // is: where it shows again when the edit is done.
                let scrub = (self.focus == focus).then_some(self.scrub).flatten();
                self.set_focus(focus.to_vec());
                self.scrub = scrub;
                self.editing = path;
                self.edit_checkpoint = Some(self.history.depth());
                self.edit_untouched = false;
                self.pick_next();
                true
            }
            Message::FinishEdit => {
                let back = self.returning.take();
                self.finish_edit();
                // The sketch drawn for a step: on with that step, which
                // now only goes with the sketch if it is cancelled.
                if let Some(back) = back {
                    self.editing = back.editing;
                    self.edit_checkpoint = back.checkpoint;
                    self.edit_added = back.added;
                    self.edit_keeps = true;
                    self.pick_next();
                }
                true
            }
            Message::CancelEdit => {
                let back = self.returning.take();
                // Where the step that goes was: the scrubber after it
                // comes back by one.
                let added_at = self
                    .editing
                    .last()
                    .filter(|_| self.edit_added)
                    .zip(self.focused_part())
                    .and_then(|(id, part)| {
                        let program = &self.workspace.part(&part).ok()?.program;
                        program.iter().position(|op| op.id() == id)
                    });
                if let (Some(at), Some(s)) = (added_at, self.scrub)
                    && at < s
                {
                    self.scrub = Some(s - 1);
                }
                match (
                    self.edit_keeps && self.edit_added,
                    self.editing.split_last(),
                ) {
                    // What was made for the step on the way stays: only
                    // the step goes.
                    (true, Some((id, _))) => {
                        let (id, part) = (id.clone(), self.focused_part());
                        if let Some(part) = part {
                            self.change(None, |ws| {
                                ws.part_mut(&part)?.program.retain(|op| op.id() != id);
                                Ok(())
                            })?;
                        }
                    }
                    _ => {
                        if let Some(depth) = self.edit_checkpoint {
                            self.history.undo_to(depth, &mut self.workspace);
                        }
                    }
                }
                self.finish_edit();
                // No sketch after all: back to the step that asked for
                // one, as it was.
                if let Some(back) = back {
                    self.editing = back.editing;
                    self.edit_checkpoint = back.checkpoint;
                    self.edit_added = back.added;
                    self.pick_next();
                }
                true
            }
            Message::Toggle(path) => {
                if !self.expanded.remove(&path) {
                    self.expanded.insert(path);
                }
                false
            }
            Message::Focus(path) => {
                let active = self.active_part.as_deref().context("no active part")?;
                locate(&self.workspace, active, &path)?;
                self.finish_edit();
                self.set_focus(path);
                true
            }
            Message::Scrub(steps) => {
                self.scrub = steps;
                true
            }
            Message::Pick(field) => {
                self.picking = field;
                self.viewport.candidates.clear();
                // Picking shows the datums hidden for being used.
                true
            }
            Message::Pointer(pointer) => return self.pointer(pointer),
            Message::Camera(movement) => {
                self.camera(movement);
                false
            }
        };
        if rebuild {
            self.rebuild();
        }
        Ok(())
    }

    /// Whether there is a change to undo.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether there is an undone change to redo.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// The part being worked on.
    pub fn active_part(&self) -> Option<&Part> {
        self.workspace.parts.get(self.active_part.as_ref()?)
    }

    /// The path of the focused part's definition: the active part, or the
    /// part placed at the focus.
    pub fn focused_part(&self) -> Option<String> {
        let active = self.active_part.as_deref()?;
        Some(locate(&self.workspace, active, &self.focus).ok()?.part)
    }

    /// What the user is told of that is no error: references that were
    /// found again under another name.
    pub fn notice(&self) -> Option<String> {
        self.notice.clone()
    }

    /// Why what is shown could not be built, if it could not and should
    /// be said: not for a step just added that the user has not changed
    /// yet (it is bound to lack references).
    pub fn error(&self) -> Option<String> {
        if self.edit_untouched {
            return None;
        }
        self.viewport.shown.error.clone()
    }

    /// The edited step's dialog and handles.
    pub(crate) fn session(&self) -> Option<std::rc::Rc<Session>> {
        edit::session(self)
    }

    /// Whether the part at `path` differs from its file (or has none).
    pub fn is_dirty(&self, path: &str) -> bool {
        // The cache counts: it is where the part's next build starts.
        self.saved.get(path) != self.workspace.parts.get(path)
    }

    /// The selected references that steps of the focused part can use:
    /// those in its own world (not in placed parts), in selection order.
    pub fn selected_sources(&self) -> Vec<Source> {
        self.selection
            .iter()
            .filter_map(|c| match c {
                Candidate::Reference { instance, source } => {
                    Some(Source::placed(instance, source.clone()))
                }
                Candidate::Handle { .. } | Candidate::Joint { .. } => None,
            })
            .collect()
    }

    /// How many steps of the focused part are shown: up to and with the
    /// edited one, or up to the scrubber, or all.
    pub fn shown_steps(&self) -> usize {
        let Some(part) = self
            .focused_part()
            .and_then(|p| self.workspace.part(&p).ok())
        else {
            return 0;
        };
        let len = part.program.len();
        let edited = self
            .editing
            .last()
            .and_then(|id| part.program.iter().position(|op| op.id() == id));
        edited.map_or_else(|| self.scrub.unwrap_or(len).min(len), |i| i + 1)
    }

    /// The datums of the part at `part` that its shown steps refer to:
    /// used, so hidden by default. Of the focused part only the steps
    /// shown count (going back in time shows what later steps hid).
    pub fn used_datums(&self, part: &str) -> BTreeSet<String> {
        let Ok(program) = self.workspace.part(part).map(|p| &p.program) else {
            return BTreeSet::new();
        };
        let steps = if self.focused_part().as_deref() == Some(part) {
            self.shown_steps()
        } else {
            program.len()
        };
        program
            .iter()
            .take(steps)
            .flat_map(|op| op.references())
            .filter_map(|source| match source {
                Source::Datum(name)
                | Source::FrameAxis { frame: name, .. }
                | Source::Profile(name) => Some(name),
                Source::Topology(_) | Source::Placed { .. } => None,
            })
            .collect()
    }

    /// Whether the datum, body or sketch `name` of the part at `part` is
    /// hidden, and whether only because later steps use it.
    pub fn hidden(&self, part: &str, name: &str, used: &BTreeSet<String>) -> (bool, bool) {
        let key = (part.to_owned(), name.to_owned());
        if self.user_hidden.contains(&key) {
            return (true, false);
        }
        // The origin always shows: everything starts from it.
        let auto = name != ORIGIN && used.contains(name) && !self.user_shown.contains(&key);
        (auto, auto)
    }

    /// What of `world`, the world of the part at `part`, is hidden. While
    /// a reference is picked, datums hidden for being used show.
    fn visibility(&self, part: &str, world: &World) -> Visibility {
        let used = if self.picking.is_some() {
            BTreeSet::new()
        } else {
            self.used_datums(part)
        };
        let hidden = |name: &str| self.hidden(part, name, &used).0;
        let bodies = world
            .topo
            .bodies()
            .ids()
            .filter_map(|b| world.topo.name_of(b));
        // A sketch being edited shows through its canvas instead.
        let edited = self
            .editing
            .last()
            .filter(|_| self.focused_part().as_deref() == Some(part));
        let profiles = world
            .profiles
            .keys()
            .filter(|n| hidden(n) || Some(*n) == edited);
        let wanted = if self.focused_part().as_deref() == Some(part) {
            self.wanted_kinds()
        } else {
            Vec::new()
        };
        Visibility {
            datums: world.datums.keys().filter(|n| hidden(n)).cloned().collect(),
            bodies: bodies.filter(|n| hidden(n)).map(str::to_owned).collect(),
            instances: BTreeSet::new(),
            profiles: profiles.cloned().collect(),
            shades: self.workspace.library.shades(),
            datum_size: self.datum_size,
            sketches_in_front: wanted.contains(&ReferenceKind::Profile),
            datums_in_front: wanted.iter().any(|kind| {
                matches!(
                    kind,
                    ReferenceKind::Plane
                        | ReferenceKind::Axis
                        | ReferenceKind::Frame
                        | ReferenceKind::Point
                )
            }),
        }
    }

    /// The kinds of reference the field being picked into takes, if one
    /// is.
    fn wanted_kinds(&self) -> Vec<ReferenceKind> {
        let Some(path) = &self.picking else {
            return Vec::new();
        };
        let Some(session) = self.session() else {
            return Vec::new();
        };
        match edit::widget_at(&session.visualization.dialog, path) {
            Some(Widget::ReferenceInput(input)) => with_planes(&input.kinds()),
            Some(Widget::ReferenceList(list)) => with_planes(&list.kinds),
            _ => Vec::new(),
        }
    }

    /// The size of a pixel in model units at `at` (in the active part).
    pub(crate) fn pixel(&self, at: Point3) -> f64 {
        let height = self.viewport.size[1];
        f64::from(self.viewport.camera.pixel_size(at.as_vec3(), height))
    }

    /// The handles of the edited step, rasterized in the active part's
    /// coordinates, the same size on screen wherever they are.
    pub fn handle_scene(&self) -> Scene {
        let mut scene = Scene::default();
        if let Some(session) = self.session() {
            let pose = self.viewport.shown.pose;
            let pixel = |at| self.pixel(at);
            let right = self.viewport.camera.right().as_dvec3();
            handles::handles(
                &mut scene,
                &session.visualization.handles,
                pose,
                &pixel,
                right,
                self.viewport.camera.up().as_dvec3(),
                self.viewport.camera.target.as_dvec3(),
            );
        }
        self.draw_snap(&mut scene);
        self.outline_picked(&mut scene);
        self.outline_hovered(&mut scene);
        if let Some(pose) = self.offered_connector() {
            self.draw_connector(&mut scene, &pose);
        }
        self.draw_joints(&mut scene);
        scene
    }

    /// Where the point moved or placed snapped to, as the sketcher shows
    /// it: a dot, and the badge of what it is on beside it; and, while
    /// the gizmo's outer ring is dragged, how much it scales by.
    fn draw_snap(&self, scene: &mut Scene) {
        use workpiece_rasterize::palette::PREVIEW;
        // (Only while a step is edited in the scene.)
        if !self.session().is_some_and(|s| s.space().is_some()) {
            return;
        }
        let pose = self.viewport.shown.pose;
        let right = self.viewport.camera.right();
        let up = self.viewport.camera.up();
        let label = |at: Point3, text: String| {
            let px = self.pixel(at) as f32;
            workpiece_rasterize::Label {
                position: at.as_vec3() + (up - right) * px * 14.0,
                along: right,
                text,
                upper: String::new(),
                lower: String::new(),
                color: PREVIEW,
                badge: true,
            }
        };
        if let Some(snap) = self.viewport.snapped {
            let at = pose.transform_point(snap.at);
            scene.front.vertices.push(workpiece_rasterize::Vertex {
                position: at.as_vec3(),
                size: 10.0,
                color: PREVIEW,
                pick: None,
            });
            scene.labels.push(label(at, snap.kind.badge().to_owned()));
        }
        let scaling = matches!(
            self.viewport.dragging,
            Some(Candidate::Handle {
                part: Some(GizmoPart::Scale),
                ..
            })
        );
        if let (true, Some(drag)) = (scaling, self.viewport.gizmo_drag) {
            let at = pose.transform_point(drag.pose.translation());
            scene.labels.push(label(at, format!("×{:.3}", drag.factor)));
        }
    }

    /// The joints the joint field being picked takes, each with its
    /// kind and its frame (its first connector's) in the active part's
    /// coordinates: where it is drawn and pointed at. None while no such
    /// field is picked.
    fn joint_glyphs(
        &self,
    ) -> Vec<(
        String,
        workpiece_core::world::JointKind,
        workpiece_core::Pose,
    )> {
        let Some(session) = self.session() else {
            return Vec::new();
        };
        let field = self
            .picking
            .as_ref()
            .and_then(|f| edit::widget_at(&session.visualization.dialog, f));
        let Some(Widget::JointInput(input)) = field else {
            return Vec::new();
        };
        let shown = &self.viewport.shown;
        shown
            .world
            .joints
            .iter()
            .filter(|j| input.takes(j.kind))
            .filter_map(|j| {
                let at = match &j.a.instance {
                    Some(name) => shown.world.instances.get(name)?.pose,
                    None => workpiece_core::Pose::IDENTITY,
                };
                Some((j.name.clone(), j.kind, shown.pose * at * j.a.frame))
            })
            .collect()
    }

    /// The joints under `ray` (in the active part's coordinates) that
    /// the joint field being picked takes, nearest first: those whose
    /// glyph the pointer is on.
    fn joint_candidates(&self, ray: &workpiece_core::Ray) -> Vec<Candidate> {
        let mut found: Vec<(f64, Candidate)> = self
            .joint_glyphs()
            .into_iter()
            .filter_map(|(name, kind, frame)| {
                let o = frame.translation();
                let along = (o - ray.start()).dot(ray.direction());
                let off = (o - ray.at(along)).length() / self.pixel(o);
                (off <= JOINT_GLYPH + 6.0).then_some((off, Candidate::Joint { name, kind }))
            })
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.into_iter().map(|(_, c)| c).collect()
    }

    /// Draws the joints the joint field being picked takes, each where
    /// it is: a ring about its axis (with arrows along it, for one that
    /// slides), the one under the pointer highlighted.
    fn draw_joints(&self, scene: &mut Scene) {
        use workpiece_rasterize::{Line, Vertex, palette};
        let hovered = match self.viewport.hovered() {
            Some(Candidate::Joint { name, .. }) => Some(name.clone()),
            _ => None,
        };
        for (name, kind, frame) in self.joint_glyphs() {
            let color = if hovered.as_ref() == Some(&name) {
                palette::HANDLE_HOVER
            } else {
                palette::REFERENCE
            };
            let width = if hovered.as_ref() == Some(&name) {
                3.5
            } else {
                2.5
            };
            let o = frame.translation();
            let pixel = self.pixel(o);
            let (x, y, z) = (
                frame.axis(workpiece_core::Axis::X),
                frame.axis(workpiece_core::Axis::Y),
                frame.axis(workpiece_core::Axis::Z),
            );
            let r = JOINT_GLYPH * pixel;
            let mut line = |a: Point3, b: Point3, w: [f32; 2]| {
                scene.front.lines.push(Line {
                    a: a.as_vec3(),
                    b: b.as_vec3(),
                    width: w,
                    color,
                    pick: None,
                });
            };
            // The ring about its axis.
            let round = |k: usize| {
                let t = std::f64::consts::TAU * k as f64 / 24.0;
                o + (x * libm::cos(t) + y * libm::sin(t)) * r
            };
            for k in 0..24 {
                line(round(k), round(k + 1), [width; 2]);
            }
            // Its axis through it; arrowheads at its ends where it
            // slides.
            let reach = z * (r * 1.8);
            line(o - reach, o + reach, [width; 2]);
            if kind.slides() {
                for end in [reach, -reach] {
                    let tip = o + end * 1.3;
                    line(o + end, tip, [width * 3.0, 0.0]);
                }
            }
            scene.front.vertices.push(Vertex {
                position: o.as_vec3(),
                size: 8.0,
                color,
                pick: None,
            });
        }
    }

    /// The part and the id of the step at the step path `path`.
    fn step_at(&self, path: &[String]) -> anyhow::Result<(String, String)> {
        let (id, focus) = path.split_last().context("no step")?;
        let active = self.active_part.as_deref().context("no active part")?;
        let part = locate(&self.workspace, active, focus)?.part;
        Ok((part, id.clone()))
    }

    /// The text field the edited step's canvas has open, placed in the
    /// active part's coordinates.
    pub fn entry(&self) -> Option<crate::view::Entry> {
        let session = self.session()?;
        let pose = self.viewport.shown.pose;
        let (_, canvas) = session.canvas()?;
        let position = pose.transform_point(canvas.place(canvas.input.as_ref()?.at));
        let input = canvas.input.clone()?;
        Some(crate::view::Entry {
            key: input.key,
            position: position.as_vec3(),
            text: input.text,
            enter: input.on_enter,
            cancel: input.on_cancel,
        })
    }

    /// The drawing that is edited, if the step that is edited is one:
    /// the part it is of, and the step.
    pub fn drawing(&self) -> Option<(String, workpiece_ops_drawing::Drawing)> {
        let part = self.focused_part()?;
        let step = self.editing.last()?;
        let op = self.workspace.part(&part).ok()?.operation(step)?;
        let drawing = op.downcast_ref::<workpiece_ops_drawing::Drawing>()?;
        Some((part, drawing.clone()))
    }

    /// The picture that is edited, if the step that is edited is one:
    /// the part it is of, and the step.
    pub fn render(&self) -> Option<(String, workpiece_ops_appearance::Render)> {
        let part = self.focused_part()?;
        let step = self.editing.last()?;
        let op = self.workspace.part(&part).ok()?.operation(step)?;
        let render = op.downcast_ref::<workpiece_ops_appearance::Render>()?;
        Some((part, render.clone()))
    }

    /// What a unit of volume of the focused part weighs, in kilograms:
    /// by the density of what it is made of, or else by its parameter
    /// `density`, if it has one.
    pub fn density(&self) -> Option<f64> {
        let path = self.focused_part()?;
        let part = self.workspace.part(&path).ok()?;
        let material = part.material.as_ref();
        if let Some(material) = material.and_then(|m| self.workspace.library.get(m)) {
            // (A cubic unit of the part, in cubic metres.)
            let cubic = (part.unit.millimetres() / 1000.0).powi(3);
            return Some(material.density * cubic);
        }
        let values = part.resolve(self.variant.as_deref()).ok()?;
        values.get("density").copied()
    }

    /// The world a sheet of the focused part is drawn from. A drawing
    /// that is edited is of the world its step is applied to: what the
    /// steps before it make, the same however its sheet changes — so
    /// what its views see (and what the part weighs) is kept while it
    /// is edited, not worked out again for every move on the sheet.
    pub fn drawn(&self) -> &std::sync::Arc<World> {
        match self.drawing() {
            Some(_) => &self.viewport.shown.before,
            None => &self.viewport.shown.world,
        }
    }

    /// What `with` makes of what a sheet of the focused part is drawn
    /// from: the model as it shows, and what the part says of itself.
    pub fn with_subject<T>(
        &self,
        with: impl FnOnce(&workpiece_ops_drawing::lay::Subject<'_>) -> T,
    ) -> Option<T> {
        let path = self.focused_part()?;
        let part = self.workspace.part(&path).ok()?;
        let material = part.material.clone();
        let name = std::path::Path::new(&path)
            .file_stem()
            .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
        let properties = self.properties();
        let parts = self.workspace.placed_names(&path);
        let densities: std::collections::BTreeMap<String, f64> = self
            .workspace
            .library
            .materials
            .iter()
            .map(|m| (m.name.clone(), m.density))
            .collect();
        // (Which of the part's sheets: its drawing steps, in order — the
        // one edited, else the first.)
        let drawings: Vec<&str> = part
            .program
            .iter()
            .filter(|op| {
                op.downcast_ref::<workpiece_ops_drawing::Drawing>()
                    .is_some()
            })
            .map(|op| op.id())
            .collect();
        let edited = self.editing.last().map(String::as_str);
        let at = drawings
            .iter()
            .position(|id| Some(*id) == edited)
            .unwrap_or(0);
        let subject = workpiece_ops_drawing::lay::Subject {
            world: self.drawn(),
            unit: part.unit,
            material,
            name: &name,
            properties: &properties,
            parts: &parts,
            densities: &densities,
            sheet: [at + 1, drawings.len().max(1)],
        };
        Some(with(&subject))
    }

    /// An extrusion or a revolution just added without a sketch takes the
    /// sketch drawn last, if no step uses it yet; its field still picks
    /// (unless another has to be picked first), so a click takes another.
    fn take_last_sketch(&mut self, part: &str, added: &str) -> anyhow::Result<()> {
        let Ok(program) = self.workspace.part(part).map(|p| &p.program) else {
            return Ok(());
        };
        let Some(at) = program.iter().position(|op| op.id() == added) else {
            return Ok(());
        };
        let (added, before) = (&program[at], &program[..at]);
        if !matches!(added.kind(), "Extrude" | "Revolve") {
            return Ok(());
        }
        let Some(sketch) = before.iter().rev().find(|op| op.kind() == "Sketch") else {
            return Ok(());
        };
        let id = sketch.id().to_owned();
        let within = format!("{id}/");
        let used = before
            .iter()
            .flat_map(|op| op.references())
            .any(|s| match s {
                Source::Profile(name) => name == id,
                Source::Datum(name) => name.starts_with(&within),
                _ => false,
            });
        if used {
            return Ok(());
        }
        let Some(session) = self.session() else {
            return Ok(());
        };
        let dialog = &session.visualization.dialog;
        let Some(field) = edit::sketch_field(dialog) else {
            return Ok(());
        };
        let message = match edit::widget_at(dialog, &field) {
            // (Not over one the selection gave.)
            Some(Widget::ReferenceInput(input)) if input.value.is_none() => {
                input.set(Source::Profile(id))
            }
            _ => return Ok(()),
        };
        self.update(message)?;
        // (Taken, not chosen: the step is as new.)
        self.edit_untouched = true;
        self.pick_next();
        if self.picking.is_none() {
            self.picking = Some(field);
        }
        Ok(())
    }

    /// Starts picking the edited step's first reference with nothing
    /// picked, if any: the focus goes where the user has to pick.
    fn pick_next(&mut self) {
        self.picking = self
            .session()
            .and_then(|s| edit::first_empty_reference(&s.visualization.dialog));
    }

    fn finish_edit(&mut self) {
        self.editing.clear();
        self.picking = None;
        self.edit_checkpoint = None;
        self.edit_untouched = false;
        self.edit_again = false;
        self.edit_added = false;
        self.edit_keeps = false;
        self.returning = None;
        self.edit_state = None;
        self.viewport.dragging = None;
        self.history.seal();
    }

    /// Focuses `path`, expanding the tree down to it and showing all of it.
    fn set_focus(&mut self, path: Vec<String>) {
        for n in 1..=path.len() {
            self.expanded.insert(path[..n].to_vec());
        }
        self.focus = path;
        self.scrub = None;
    }

    /// Applies `f` to the workspace and records the change under `key`;
    /// rolls back if `f` fails. A change that changes nothing is not
    /// recorded.
    fn change(
        &mut self,
        key: Option<EditKey>,
        f: impl FnOnce(&mut Workspace) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let before = self.workspace.clone();
        if let Err(e) = f(&mut self.workspace) {
            self.workspace = before;
            return Err(e);
        }
        if self.workspace != before {
            self.history.record(before, key);
        }
        Ok(())
    }

    /// Builds what the viewport shows again, after dropping a focus or an
    /// edit that no longer exists (after an undo, say).
    ///
    /// With [`background`](Self::background) on, the build runs on the
    /// engine's threads and what it makes shows once [`poll`](Self::poll)
    /// finds it done; a build still running for what the model was
    /// before is told to stop. The interface is never kept waiting: an
    /// edit can be cancelled, or its preview turned off, while a part is
    /// building.
    fn rebuild(&mut self) {
        let background = self.background && self.engine.pool().is_some();
        self.rebuild_on(background);
    }

    /// [`rebuild`](Self::rebuild), on this thread whatever the model is
    /// set to: for what must show at once and builds nothing new (a part
    /// moved by the pointer).
    fn rebuild_now(&mut self) {
        self.rebuild_on(false);
    }

    fn rebuild_on(&mut self, background: bool) {
        if let Some(pending) = self.pending.take() {
            pending.cancel.cancel();
        }
        let Some(active) = self.active_part.clone() else {
            self.viewport.shown = Default::default();
            return;
        };
        if locate(&self.workspace, &active, &self.focus).is_err() {
            self.focus.clear();
            self.finish_edit();
        }
        if !self.editing.is_empty() && self.session().is_none() {
            self.finish_edit();
        }
        // What the build is of: the model as it is now, without what it
        // shows or remembers.
        let of = Model {
            workspace: self.workspace.clone(),
            active_part: self.active_part.clone(),
            editing: self.editing.clone(),
            focus: self.focus.clone(),
            scrub: self.scrub,
            variant: self.variant.clone(),
            picking: self.picking.clone(),
            user_hidden: self.user_hidden.clone(),
            user_shown: self.user_shown.clone(),
            display: self.display.clone(),
            settings: self.settings.clone(),
            preview_disabled: self.preview_disabled,
            datum_size: self.datum_size,
            engine: self.engine.clone(),
            scenes: self.scenes.clone(),
            ..Model::default()
        };
        let cancel = workpiece_part::Cancel::new();
        let build = {
            let cancel = cancel.clone();
            move || {
                let workspace = std::sync::Arc::new(of.workspace.clone());
                let visibility = |part: &str, world: &World| of.visibility(part, world);
                viewport::shown(
                    &of.scenes,
                    &of.engine,
                    &cancel,
                    &workspace,
                    &active,
                    &of.focus,
                    of.editing.last().map(String::as_str),
                    of.scrub,
                    !of.preview_disabled,
                    of.variant.as_deref(),
                    &visibility,
                    of.display.clone(),
                )
            }
        };
        match self.engine.pool().filter(|_| background) {
            Some(pool) => {
                let (sender, receiver) = std::sync::mpsc::channel();
                let under = cancel.clone();
                pool.spawn(move || {
                    // Nobody waits for a build that was cancelled; one
                    // that broke is shown as that.
                    let shown = match under.run(build) {
                        Ok(shown) => shown,
                        Err(workpiece_core::Stopped::Cancelled) => return,
                        Err(broke) => viewport::Shown {
                            error: Some(format!("The part could not be shown: {broke}")),
                            ..Default::default()
                        },
                    };
                    let _ = sender.send(shown);
                });
                self.pending = Some(Pending {
                    cancel,
                    done: std::sync::Arc::new(std::sync::Mutex::new(receiver)),
                });
            }
            None => {
                let shown = build();
                self.show(shown);
            }
        }
    }

    /// Whether a build is running whose result is not shown yet.
    pub fn is_building(&self) -> bool {
        self.pending.is_some()
    }

    /// Shows what a background build made, if one is done; says whether
    /// one is still running (so that whoever draws the model looks
    /// again).
    pub fn poll(&mut self) -> bool {
        let Some(pending) = &self.pending else {
            return false;
        };
        let done = match pending.done.lock() {
            Ok(receiver) => receiver.try_recv(),
            Err(_) => Err(std::sync::mpsc::TryRecvError::Disconnected),
        };
        match done {
            Ok(shown) => {
                self.pending = None;
                self.show(shown);
                // (Showing it may start another build.)
                self.pending.is_some()
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                false
            }
        }
    }

    /// The unit of length the focused part's numbers are in.
    pub fn unit(&self) -> workpiece_core::Unit {
        self.focused_part()
            .and_then(|part| self.workspace.part(&part).ok().map(|p| p.unit))
            .unwrap_or_default()
    }

    /// Takes what a build made: shows it, and gives what it worked out
    /// to the part.
    fn show(&mut self, shown: viewport::Shown) {
        // What the build worked out goes to the part: where its next
        // build starts. It follows from a change the user made (or from
        // the part never having been built), so it is no undo step of
        // its own; undoing that change brings the old cache back with it.
        if let (Some(cache), Some(part)) = (&shown.cache, self.focused_part())
            && let Ok(part) = self.workspace.part_mut(&part)
            && part.cache != *cache
        {
            part.cache = cache.clone();
        }
        // What a step referred to by a name that is no more, and the
        // build found again where it was: the step refers to that from
        // now on, and the user is told.
        if !shown.repaired.is_empty()
            && let Some(part) = self.focused_part()
            && let Ok(part) = self.workspace.part_mut(&part)
        {
            let mut told = Vec::new();
            for repair in &shown.repaired {
                let step = part.program.iter_mut().find(|op| op.id() == repair.step);
                if let Some(step) = step {
                    step.retarget(&repair.from, &repair.to);
                    told.push(format!(
                        "{}: {:?} is no more; it now refers to {:?}, which is where that was",
                        repair.step,
                        repair.from.name(),
                        repair.to.name()
                    ));
                }
            }
            if !told.is_empty() {
                self.notice = Some(told.join("\n"));
            }
        }
        self.viewport.shown = shown;
        self.viewport.candidates.clear();
        // (A dimension dropped shows where the build put it now.)
        if self.held_note.as_ref().is_some_and(|h| h.dropped) {
            self.held_note = None;
        }
        self.face_edit_plane();
    }

    /// Turns the view to face the plane the edited step is edited in (a
    /// sketch's), when it starts being edited there.
    fn face_edit_plane(&mut self) {
        // A picture that starts being edited shows from where it was
        // taken.
        let seen = self.camera_view();
        let shot = (self.render().map(|(_, render)| render.id))
            .or_else(|| seen.as_ref().map(|(_, view)| view.id.clone()));
        if shot != self.edit_shot {
            let taken = (self.render().and_then(|(_, render)| render.args.view))
                .or_else(|| seen.as_ref().and_then(|(_, view)| view.args.view));
            if let Some(view) = taken {
                self.fly(crate::view::camera_of(&view));
            } else if let Some((_, view)) = &seen
                && self.viewport.shown.world.views.contains_key(&view.id)
            {
                let _ = self.look_from(&view.id.clone());
            }
        }
        self.edit_shot = shot;
        let plane = edit::edit_plane(self);
        if let Some(plane) = plane.filter(|p| self.edit_plane != Some(*p)) {
            self.face(&plane);
        }
        self.edit_plane = plane;
        if self.cut_at_sketch(plane.as_ref()) {
            self.rebuild();
        }
    }

    /// Cuts the model open at the plane a sketch is drawn on, as long
    /// as it is, and puts the display back when the sketch is left;
    /// says whether the display changed. The cut takes away the side
    /// the view looks from. What the user chose meanwhile (the cut put
    /// away, another one) stays until the sketch is left.
    fn cut_at_sketch(&mut self, plane: Option<&DatumPlane>) -> bool {
        use workpiece_core::world::Cutting;
        let Some(plane) = plane.filter(|_| self.settings.sketch_section) else {
            // Left: back to what it was, unless the sketch never cut.
            return match self.sketch_section.take() {
                Some((before, _)) => std::mem::replace(&mut self.display, before) != self.display,
                None => false,
            };
        };
        let pose = self.viewport.shown.pose;
        let camera = self
            .viewport
            .transition
            .map_or(self.viewport.camera, |t| t.to);
        let normal = plane.normal();
        let toward_camera = pose.transform_vector(normal).dot(camera.back().as_dvec3());
        let normal = if toward_camera < 0.0 { -normal } else { normal };
        let cutting = Cutting::plane(plane.origin(), normal);
        match &mut self.sketch_section {
            // From one plane to the next: the cut follows, if it is
            // still the sketch's.
            Some((_, made)) => {
                if self.display.plane != *made || *made == Some(cutting.clone()) {
                    return false;
                }
                *made = Some(cutting.clone());
            }
            none => *none = Some((self.display.clone(), Some(cutting.clone()))),
        }
        self.display.cut = None;
        self.display.plane = Some(cutting);
        true
    }

    /// Sends the camera to face `plane` (of the focused part) square on,
    /// by the smallest turn that does: from the side it is seen from
    /// now, with whichever of the plane's axes is nearest to upright on
    /// the screen left upright (either way round) — nothing is turned on
    /// its head. What is horizontal in the sketch is then what the view
    /// shows as level.
    fn face(&mut self, plane: &DatumPlane) {
        let pose = self.viewport.shown.pose;
        let base = self
            .viewport
            .transition
            .map_or(self.viewport.camera, |t| t.to);
        let (origin, normal) = (
            pose.transform_point(plane.origin()),
            pose.transform_vector(plane.normal()),
        );
        // Looking at the same spot, dropped onto the plane.
        let target = base.target.as_dvec3();
        let target = target - normal * (target - origin).dot(normal);
        let (x, y) = (
            pose.transform_vector(plane.x_dir()).as_vec3(),
            pose.transform_vector(plane.y_dir()).as_vec3(),
        );
        // Of the eight ways to look at it square on, the nearest. (Seen
        // just as squarely from either side, it is seen from its front,
        // its y up.)
        let normal = normal.as_vec3();
        let mut nearest = (f32::NEG_INFINITY, base);
        for back in [normal, -normal] {
            for up in [y, -y, x, -x] {
                let to = base.facing(target.as_vec3(), back, up);
                let near = to.rotation.dot(base.rotation).abs();
                if near > nearest.0 + 1e-5 {
                    nearest = (near, to);
                }
            }
        }
        self.fly(nearest.1);
    }

    /// Sends the camera on its way to `to`.
    fn fly(&mut self, to: Camera) {
        let from = self.viewport.camera;
        self.viewport.transition = (to != from).then_some(Transition {
            from,
            to,
            progress: 0.0,
        });
    }

    /// Sends `kind` at `at` to the edited step's canvas, if it has one
    /// and no reference is being picked. Returns whether there is a
    /// canvas to take it.
    fn canvas_event(
        &mut self,
        kind: CanvasEventKind,
        at: PointerAt,
        add: bool,
    ) -> anyhow::Result<bool> {
        if self.picking.is_some() {
            return Ok(false);
        }
        let Some(session) = self.session() else {
            return Ok(false);
        };
        let pose = self.viewport.shown.pose;
        // A space takes the event as the ray it is.
        if let Some((_, space)) = session.space() {
            let ray = self
                .ray(at)
                .and_then(|ray| picking::place(&ray, pose.inverse()));
            let Some(ray) = ray else {
                return Ok(true);
            };
            let local = pose.inverse();
            let camera = &self.viewport.camera;
            let view = workpiece_core::handles::SpaceView {
                right: local.transform_vector(camera.right().as_dvec3()),
                up: local.transform_vector(camera.up().as_dvec3()),
                pixel: self.pixel(pose.transform_point(space.anchor)),
            };
            // What the pointer snaps to: while something is dragged,
            // or always for a space that places points.
            let wants = space.placing || kind == CanvasEventKind::Drag;
            let snap = if wants {
                self.snap_at(&space.targets, &ray, None)
            } else {
                None
            };
            let grid = (wants && self.snap_kinds().takes(SnapKind::Grid))
                .then(|| workpiece_core::handles::snap::grid_step(view.pixel));
            self.viewport.snapped = snap;
            let message = space.event(workpiece_core::handles::SpaceEvent {
                kind,
                ray,
                add,
                view,
                snap,
                grid,
            });
            if let Some(message) = message {
                self.update(message)?;
            }
            return Ok(true);
        }
        let Some((_, canvas)) = session.canvas() else {
            return Ok(false);
        };
        let ray = self
            .ray(at)
            .and_then(|ray| picking::place(&ray, pose.inverse()));
        let Some(point) = ray.and_then(|ray| canvas.hit(&ray)) else {
            return Ok(true);
        };
        let pixel = self.pixel(pose.transform_point(canvas.place(point)));
        let right = self.viewport.camera.right().as_dvec3();
        let up = self.viewport.camera.up().as_dvec3();
        let middle = self.viewport.camera.target.as_dvec3();
        let view = handles::canvas_view(canvas, pose, right, up, pixel, middle);
        let message = canvas.event(CanvasEvent {
            kind,
            at: point,
            add,
            view,
        });
        if let Some(message) = message {
            self.update(message)?;
        }
        Ok(true)
    }

    /// The name of the edited step's canvas, while it takes the pointer.
    fn canvas_name(&self) -> Option<String> {
        if self.picking.is_some() {
            return None;
        }
        let session = self.session()?;
        let canvas = session.canvas().map(|(name, _)| name.to_owned());
        canvas.or_else(|| session.space().map(|(name, _)| name.to_owned()))
    }

    /// The edited step's handles under the pointer at `at`.
    fn grips(&self, at: PointerAt) -> Vec<Candidate> {
        let (Some(session), Some(ray)) = (self.session(), self.ray(at)) else {
            return Vec::new();
        };
        let pixel = |at| self.pixel(at);
        let handles = &session.visualization.handles;
        picking::handles(handles, self.viewport.shown.pose, &ray, &pixel)
    }

    /// Whether the pointer is on one of the edited step's handles.
    fn on_grip(&self) -> bool {
        matches!(self.viewport.hovered(), Some(Candidate::Handle { .. }))
    }

    /// What is under the pointer, as a pick for a connector: what it
    /// is, and where on it the pointer is. Connectors are picked in the
    /// world as it is with the edited step: a part just added is there
    /// to be pointed at.
    fn picked(&self) -> Option<workpiece_core::widgets::Picked> {
        let Candidate::Reference { instance, source } = self.viewport.hovered()? else {
            return None;
        };
        let shown = &self.viewport.shown;
        let ray = picking::place(&self.ray(self.viewport.pointer?)?, shown.pose.inverse())?;
        let at = picking::hit_point(&shown.world, &ray, instance, source)?;
        // Twelve pixels, where the pointer is.
        let mut pose = shown.pose;
        let mut within = shown.world.clone();
        for name in instance {
            let placed = within.instances.get(name)?;
            pose = pose * placed.pose;
            let inner = placed.world.clone();
            within = inner;
        }
        Some(workpiece_core::widgets::Picked {
            instance: instance.clone(),
            source: source.clone(),
            at,
            near: self.pixel(pose.transform_point(at)) * 12.0,
            within,
        })
    }

    /// The connector field being picked, if one is: what it takes, and
    /// which pair of the step's connectors it is one of.
    fn connector_field(&self) -> Option<(workpiece_core::widgets::Scope, usize)> {
        let session = self.session()?;
        match edit::widget_at(&session.visualization.dialog, self.picking.as_ref()?)? {
            Widget::ConnectorInput(input) => Some((input.scope.clone(), input.pair)),
            _ => None,
        }
    }

    /// The connector the pointer offers, while a joint's connector is
    /// being picked.
    fn offered_anchor(&self) -> Option<workpiece_ops_instances::Anchor> {
        self.connector_field()?;
        workpiece_ops_instances::Anchor::picked(&self.picked()?)
    }

    /// The frame of the connector the pointer offers: where it is in
    /// the view.
    fn offered_connector(&self) -> Option<workpiece_core::Pose> {
        let shown = &self.viewport.shown;
        let frame = self.offered_anchor()?.frame_in(&shown.world).ok()?;
        Some(shown.pose * frame)
    }

    /// The part being added as it would lie with the connector the
    /// pointer offers: drawn faintly where its joint would bring it,
    /// before anything is picked. Nothing, unless the other connector of
    /// the joint is chosen already.
    pub fn joint_preview(&self) -> workpiece_rasterize::Drawing {
        let none = workpiece_rasterize::Drawing::default();
        let (Some((scope, pair)), Some(session)) = (self.connector_field(), self.session()) else {
            return none;
        };
        // (Of a part of the project, or one out of the standards.)
        let op = &session.op;
        let hold = op
            .downcast_ref::<workpiece_ops_instances::AddPart>()
            .map(|add| &add.args.hold)
            .or_else(|| {
                op.downcast_ref::<workpiece_ops_standard::StandardPart>()
                    .map(|standard| &standard.args.hold)
            });
        // (The mate whose reference is picked.)
        let mate = hold.and_then(|hold| hold.mates.get(pair));
        let (Some(mate), Some(offered)) = (mate, self.offered_anchor()) else {
            return none;
        };
        let shown = &self.viewport.shown;
        let Some(placed) = shown.world.instances.get(op.id()) else {
            return none;
        };
        // The connector on the assembly, and the one on the part (told
        // from inside it): one of them chosen, the other offered.
        let on_part = matches!(scope, workpiece_core::widgets::Scope::Inside(_));
        let (onto, own) = if on_part {
            (mate.onto.clone(), Some(offered.inside()))
        } else {
            (Some(offered), mate.with.clone())
        };
        let (Some(onto), Some(own)) = (onto, own) else {
            return none;
        };
        let (Ok(onto), Ok(own)) = (onto.frame_in(&shown.world), own.frame_in(&placed.world)) else {
            return none;
        };
        let pose = shown.pose * mate.onto_frame(onto) * mate.with_frame(own).inverse();
        self.scenes.draw(
            &placed.world,
            pose,
            workpiece_rasterize::Style::Ghost,
            &Visibility::placed(&placed.world, &self.workspace.library.shades()),
        )
    }

    /// Draws the frame `pose` in `scene`, the same size on screen
    /// wherever it is: its origin, and its axes in their colours, z the
    /// longest.
    fn draw_connector(&self, scene: &mut Scene, pose: &workpiece_core::Pose) {
        use workpiece_rasterize::palette;
        let o = pose.translation();
        let pixel = self.pixel(o);
        for (axis, color, reach) in [
            (workpiece_core::Axis::X, palette::X, 26.0),
            (workpiece_core::Axis::Y, palette::Y, 26.0),
            (workpiece_core::Axis::Z, palette::Z, 44.0),
        ] {
            scene.front.lines.push(workpiece_rasterize::Line {
                a: o.as_vec3(),
                b: (o + pose.axis(axis) * (pixel * reach)).as_vec3(),
                width: [3.0, 3.0],
                color,
                pick: None,
            });
        }
        scene.front.vertices.push(workpiece_rasterize::Vertex {
            position: o.as_vec3(),
            size: 11.0,
            color: palette::HANDLE_HOVER,
            pick: None,
        });
    }

    /// Outlines the sketch or the body under the pointer in `scene`:
    /// neither is one thing the scene draws, to highlight by itself.
    fn outline_hovered(&self, scene: &mut Scene) {
        let Some(Candidate::Reference { instance, source }) = self.viewport.hovered() else {
            return;
        };
        if !instance.is_empty() {
            return;
        }
        let shown = &self.viewport.shown;
        // References are picked in the world before the edited step.
        let world = if self.picking.is_some() {
            &shown.before
        } else {
            &shown.world
        };
        let mut lines: Vec<Vec<Point3>> = Vec::new();
        match source {
            Source::Profile(name) => {
                let Some(profile) = world.profiles.get(name) else {
                    return;
                };
                let pose = profile.plane.pose();
                lines.extend(profile.edges.iter().map(|e| {
                    let points = e.curve.polyline(e.t0, e.t1);
                    let place =
                        |p: &workpiece_core::math::Point2| pose.transform_point(p.extend(0.0));
                    points.iter().map(place).collect()
                }));
            }
            Source::Topology(name) => {
                let topo = &world.topo;
                let Some(body) = topo.resolve::<workpiece_core::world::topo::BodyId>(name) else {
                    // An edge or a face picked in the world before the
                    // step is drawn from there: what shows is the world
                    // with the step, where it may be gone (an edge its
                    // fillet took).
                    if self.picking.is_some() {
                        lines.extend(outline_of(world, name));
                        self.draw_outline(
                            scene,
                            &lines,
                            workpiece_rasterize::palette::HANDLE_HOVER,
                        );
                    }
                    return;
                };
                let hide_others = Visibility {
                    bodies: topo
                        .bodies()
                        .ids()
                        .filter(|&b| b != body)
                        .filter_map(|b| topo.name_of(b).map(str::to_owned))
                        .collect(),
                    ..Visibility::default()
                };
                let shown = workpiece_core::world::visibility::shown_topology(topo, &hide_others);
                let of_body: BTreeSet<_> = topo
                    .body_faces(body)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|f| topo.face(f))
                    .flat_map(|f| &f.loops)
                    .filter_map(|&l| topo.loop_(l))
                    .flat_map(|l| &l.coedges)
                    .filter_map(|&c| match topo.coedge(c)?.kind {
                        workpiece_core::world::topo::CoedgeKind::Edge { edge, .. } => Some(edge),
                        _ => None,
                    })
                    .collect();
                for edge in shown.edges.iter().filter(|e| of_body.contains(e)) {
                    if let Some((edge, curve)) = topo
                        .edge(*edge)
                        .and_then(|e| Some((e, topo.curve(e.curve)?)))
                    {
                        lines.push(curve.polyline(edge.t0, edge.t1));
                    }
                }
            }
            _ => return,
        }
        self.draw_outline(scene, &lines, workpiece_rasterize::palette::HANDLE_HOVER);
    }

    /// Draws `lines` (in the focused part's coordinates) over the scene.
    fn draw_outline(&self, scene: &mut Scene, lines: &[Vec<Point3>], color: [f32; 4]) {
        let pose = self.viewport.shown.pose;
        let at = |p: Point3| pose.transform_point(p).as_vec3();
        for pair in lines.iter().flat_map(|points| points.windows(2)) {
            scene.lines.lines.push(workpiece_rasterize::Line {
                a: at(pair[0]),
                b: at(pair[1]),
                width: [3.2; 2],
                color,
                pick: None,
            });
        }
    }

    /// Outlines what the box of references being picked holds, from the
    /// world before the step: the edges a fillet is of are gone from
    /// the world that shows. And, all the while the step is edited,
    /// what it takes along with what is picked (a blend's tangent
    /// chains), in a colour of its own.
    fn outline_picked(&self, scene: &mut Scene) {
        let Some(session) = self.session() else {
            return;
        };
        let world = &self.viewport.shown.before;
        // (Worked out once for the session: it is kept while the world
        // before the step is the same.)
        let implied = session.implied.get_or_init(|| {
            session
                .visualization
                .implied
                .iter()
                .flat_map(|name| outline_of(world, name))
                .collect()
        });
        self.draw_outline(scene, implied, workpiece_rasterize::palette::IMPLIED);
        let Some(field) = &self.picking else {
            return;
        };
        let Some(Widget::ReferenceList(list)) =
            edit::widget_at(&session.visualization.dialog, field)
        else {
            return;
        };
        let lines: Vec<Vec<Point3>> = list
            .values
            .iter()
            .filter(|s| s.instance().is_none())
            .flat_map(|s| outline_of(world, s.name()))
            .collect();
        self.draw_outline(scene, &lines, workpiece_rasterize::palette::SELECTION);
    }

    // ---- the viewport ---------------------------------------------------

    fn ray(&self, at: PointerAt) -> Option<Ray> {
        let [x, y] = at.position;
        let [w, h] = at.size;
        self.viewport.camera.ray(Vec2::new(x, y), Vec2::new(w, h))
    }

    fn camera(&mut self, movement: CameraMove) {
        if movement == CameraMove::FacePlane {
            if let Some(plane) = edit::edit_plane(self) {
                self.face(&plane);
            }
            return;
        }
        // Views the user asks for are flown to; from where a flight is
        // going, if one is under way.
        let mut to = self
            .viewport
            .transition
            .map_or(self.viewport.camera, |t| t.to);
        match movement {
            CameraMove::LookFrom(back) => to.look_from(Vec3::from(back)),
            CameraMove::ToggleProjection => to.orthographic = !to.orthographic,
            CameraMove::Home => {
                let extent = self.viewport.shown.world.extent();
                to.home(extent.map(|(lo, hi)| (lo.as_vec3(), hi.as_vec3())));
            }
            CameraMove::FacePlane => {}
            CameraMove::Orbit(_) | CameraMove::Pan(_) | CameraMove::Zoom(_) => {
                // Moving the view by hand ends a flight where it is.
                if let Some(transition) = self.viewport.transition.take() {
                    self.viewport.camera = self.viewport.camera.settled(&transition.to);
                }
                let camera = &mut self.viewport.camera;
                match movement {
                    CameraMove::Orbit([x, y]) => camera.orbit(Vec2::new(x, y)),
                    CameraMove::Pan([x, y]) => camera.pan(Vec2::new(x, y), self.viewport.size[1]),
                    CameraMove::Zoom(delta) => camera.zoom(delta),
                    _ => {}
                }
                return;
            }
        }
        self.fly(to);
    }

    fn pointer(&mut self, pointer: Pointer) -> anyhow::Result<()> {
        match pointer {
            Pointer::Moved { position, size } => {
                self.viewport.size = size;
                let at = PointerAt { position, size };
                let from = self.viewport.pointer.replace(at);
                if self.viewport.dragging.is_some() {
                    if let Some(from) = from {
                        return self.drag(from, at);
                    }
                    return Ok(());
                }
                if self.viewport.moving.is_some() {
                    return self.move_part(at);
                }
                // A canvas takes the pointer whole, but for the step's
                // other handles on it: those are grabbed first.
                if self.canvas_name().is_some() {
                    let grips = self.grips(at);
                    if !grips.is_empty() {
                        self.viewport.candidate = 0;
                        self.viewport.candidates = grips;
                        return Ok(());
                    }
                }
                if self.canvas_event(CanvasEventKind::Hover, at, false)? {
                    self.viewport.candidates.clear();
                    return Ok(());
                }
                self.hover(at);
            }
            Pointer::Left => {
                self.viewport.pointer = None;
                self.viewport.candidates.clear();
            }
            Pointer::Cycle => {
                let n = self.viewport.candidates.len().max(1);
                self.viewport.candidate = (self.viewport.candidate + 1) % n;
            }
            Pointer::Click { add } => {
                // The pointer is at work in the scene, not the timeline.
                self.step = None;
                if let Some(at) = self.viewport.pointer
                    && !self.on_grip()
                    && self.canvas_event(CanvasEventKind::Click, at, add)?
                {
                    // Each click is its own undo step.
                    self.history.seal();
                    return Ok(());
                }
                return self.confirm(add);
            }
            Pointer::DoubleClick => {
                if let Some(at) = self.viewport.pointer {
                    self.canvas_event(CanvasEventKind::DoubleClick, at, false)?;
                    self.history.seal();
                }
            }
            Pointer::Press => {
                if let (Some(name), Some(at), false) =
                    (self.canvas_name(), self.viewport.pointer, self.on_grip())
                {
                    self.canvas_event(CanvasEventKind::Press, at, false)?;
                    let canvas = Candidate::Handle { name, part: None };
                    self.viewport.pressed = Some((canvas, at));
                    return Ok(());
                }
                let hovered = self.viewport.hovered().cloned();
                // While a step is edited, what a drag moves is the placed
                // part under the pointer, whatever the field being picked
                // takes (a part being placed, while what it goes onto is
                // picked); a handle still comes first.
                let hovered = match hovered {
                    Some(handle @ Candidate::Handle { .. }) => Some(handle),
                    other if !self.editing.is_empty() => self.placed_under_pointer().or(other),
                    other => other,
                };
                self.viewport.pressed = hovered.zip(self.viewport.pointer);
            }
            Pointer::DragStart => {
                // What was pressed, not what is under the pointer now: by
                // the time a drag starts the pointer has moved, and hovering
                // there starts again at the first candidate.
                match self.viewport.pressed.take() {
                    Some((pick @ Candidate::Handle { .. }, at)) => {
                        if let Candidate::Handle { name, .. } = &pick {
                            let pose = self.session().and_then(|session| {
                                match session.visualization.handles.get(name) {
                                    Some(Handle::Gizmo(g)) => Some(g.pose),
                                    _ => None,
                                }
                            });
                            self.viewport.gizmo_drag = pose.map(|pose| viewport::GizmoDrag {
                                pose,
                                at,
                                factor: 1.0,
                            });
                        }
                        self.viewport.dragging = Some(pick);
                        // Drag from where the button went down.
                        if let Some(now) = self.viewport.pointer {
                            return self.drag(at, now);
                        }
                    }
                    // A placed part: it moves with the pointer, as far as
                    // its mates and joints let it — while a step is edited
                    // too (a part being placed is dragged where it goes).
                    Some((Candidate::Reference { instance, source }, at))
                        if !instance.is_empty() =>
                    {
                        self.viewport.moving = self.grab_part(&instance, &source, at);
                        if let (Some(_), Some(now)) = (&self.viewport.moving, self.viewport.pointer)
                        {
                            return self.move_part(now);
                        }
                    }
                    _ => {}
                }
            }
            Pointer::DragEnd if self.viewport.moving.take().is_some() => {
                self.history.seal();
            }
            Pointer::DragEnd => {
                self.viewport.gizmo_drag = None;
                self.viewport.snapped = None;
                if self.viewport.dragging.take().is_some() {
                    if let Some(at) = self.viewport.pointer {
                        self.canvas_event(CanvasEventKind::Release, at, false)?;
                    }
                    self.history.seal();
                }
            }
        }
        Ok(())
    }

    /// What of a placed part is under the pointer, in the world that
    /// shows: what a drag that starts there moves.
    fn placed_under_pointer(&self) -> Option<Candidate> {
        let shown = &self.viewport.shown;
        let ray = self.ray(self.viewport.pointer?)?;
        picking::references(&shown.world, shown.pose, &shown.visibility, &ray, &[])
            .into_iter()
            .find(|c| matches!(c, Candidate::Reference { instance, .. } if !instance.is_empty()))
    }

    /// What moves when the pointer drags `source` of the instance at
    /// `instance` (names from the shown world down), grabbed at `at`: the
    /// deepest of those instances that is not fixed, in the assembly it
    /// is placed in. A part that is fixed moves the assembly around it,
    /// up to the first that is free; nothing, if all are fixed.
    fn grab_part(
        &self,
        instance: &[String],
        source: &Source,
        at: PointerAt,
    ) -> Option<viewport::Moving> {
        let shown = &self.viewport.shown;
        let ray = picking::place(&self.ray(at)?, shown.pose.inverse())?;
        let hit = picking::hit_point(&shown.world, &ray, instance, source)?;
        // The worlds down the path, and where each is in the view.
        let mut worlds: Vec<&World> = vec![&shown.world];
        let mut poses = vec![shown.pose];
        for name in instance {
            let placed = worlds.last()?.instances.get(name)?;
            poses.push(*poses.last()? * placed.pose);
            worlds.push(&placed.world);
        }
        let grabbed = poses.last()?.transform_point(hit);
        let level = (1..=instance.len())
            .rev()
            .find(|&d| !worlds[d - 1].instances[&instance[d - 1]].fixed)?;
        let active = self.active_part.as_deref()?;
        let mut path = self.focus.clone();
        path.extend_from_slice(&instance[..level - 1]);
        let part = locate(&self.workspace, active, &path).ok()?.part;
        // Only a part of this project keeps where its instances are.
        self.workspace
            .parts
            .contains_key(&part)
            .then(|| viewport::Moving {
                part,
                within: instance[..level - 1].to_vec(),
                instance: instance[level - 1].clone(),
                local: poses[level].inverse().transform_point(grabbed),
                grabbed,
            })
    }

    /// Moves the grabbed part to the pointer at `at`: solves the joints
    /// of the assembly it is in with the part pulled there, and keeps
    /// where everything ended up with that assembly's part. Nothing is
    /// built again but that: the steps and what is drawn of every part
    /// are kept.
    fn move_part(&mut self, at: PointerAt) -> anyhow::Result<()> {
        let Some(moving) = self.viewport.moving.clone() else {
            return Ok(());
        };
        // While a part is still building there is nothing to move yet.
        if self.poll() {
            return Ok(());
        }
        let shown = &self.viewport.shown;
        let Some(ray) = self.ray(at) else {
            return Ok(());
        };
        // In the plane through where it was grabbed, facing the view.
        let normal = self.viewport.camera.back().as_dvec3();
        let Some(t) = ray.intersect_plane(moving.grabbed, normal) else {
            return Ok(());
        };
        let mut world: &World = &shown.world;
        let mut pose = shown.pose;
        for name in &moving.within {
            let Some(placed) = world.instances.get(name) else {
                return Ok(());
            };
            pose = pose * placed.pose;
            world = &placed.world;
        }
        let drag = workpiece_ops_instances::assembly::Drag {
            instance: moving.instance.clone(),
            local: moving.local,
            target: pose.inverse().transform_point(ray.at(t)),
        };
        let solved = workpiece_ops_instances::assembly::solve(world, Some(&drag));
        if solved.poses.is_empty() {
            return Ok(());
        }
        let key = ("move".to_owned(), moving.instance.clone());
        self.change(Some(key), |ws| {
            let part = ws.part_mut(&moving.part)?;
            let put: BTreeMap<String, workpiece_core::Pose> = solved
                .poses
                .iter()
                .map(|(name, pose)| (name.clone(), *pose))
                .collect();
            workpiece_ops_instances::assembly::keep(&mut part.cache, &put);
            Ok(())
        })?;
        // At once: every step and every scene is kept, only where the
        // parts are is new.
        self.rebuild_now();
        Ok(())
    }

    /// Finds the candidates under the pointer, on the model: the edited
    /// step's handles, then what the reference field being picked takes;
    /// or (not editing) any reference. Moving starts again at the first
    /// candidate: what cycling chose holds only while the pointer rests.
    fn hover(&mut self, at: PointerAt) {
        let Some(ray) = self.ray(at) else {
            return;
        };
        let shown = &self.viewport.shown;
        let candidates = match (self.session(), &self.picking) {
            (Some(session), Some(field)) => {
                // The handles stay draggable while picking: they come
                // first.
                let pixel = |at| self.pixel(at);
                let mut handles =
                    picking::handles(&session.visualization.handles, shown.pose, &ray, &pixel);
                let field = edit::widget_at(&session.visualization.dialog, field);
                let references = match field {
                    // References resolve in the world before the step.
                    // (Where a sketch is asked for, a plane will do: a
                    // sketch is drawn on it first.)
                    Some(Widget::ReferenceInput(input)) => picking::references(
                        &shown.before,
                        shown.pose,
                        &shown.visibility,
                        &ray,
                        &with_planes(&input.kinds()),
                    ),
                    // A connector sits on anything, of the placed
                    // parts its field takes: in the world with the
                    // edited step, where a part just added is.
                    // The joints, at their glyphs.
                    Some(Widget::JointInput(_)) => self.joint_candidates(&ray),
                    Some(Widget::ConnectorInput(input)) => {
                        let mut found = picking::references(
                            &shown.world,
                            shown.pose,
                            &shown.visibility,
                            &ray,
                            &[],
                        );
                        found.retain(|c| match c {
                            Candidate::Reference { instance, .. } => input.scope.takes(instance),
                            Candidate::Handle { .. } | Candidate::Joint { .. } => false,
                        });
                        found
                    }
                    // Any kind the box takes.
                    Some(Widget::ReferenceList(list)) => picking::references(
                        &shown.before,
                        shown.pose,
                        &shown.visibility,
                        &ray,
                        &with_planes(&list.kinds),
                    ),
                    _ => Vec::new(),
                };
                // Of the world before the step, only what the world
                // that shows still has can be pointed at — and what the
                // field holds already (the edges a fillet is of), to be
                // taken out of it again. What the step did away with
                // besides is not there to see.
                let held: Vec<&str> = match field {
                    Some(Widget::ReferenceList(list)) => list
                        .values
                        .iter()
                        .filter(|s| s.instance().is_none())
                        .map(|s| s.name())
                        .collect(),
                    _ => Vec::new(),
                };
                let mut references = references;
                if !matches!(field, Some(Widget::ConnectorInput(_))) {
                    references.retain(|c| match c {
                        Candidate::Reference {
                            instance,
                            source: Source::Topology(name),
                        } if instance.is_empty() => {
                            held.contains(&name.as_str())
                                || still_there(&shown.before, &shown.world, name)
                        }
                        _ => true,
                    });
                }
                handles.extend(references);
                handles
            }
            (Some(session), None) => {
                let pixel = |at| self.pixel(at);
                picking::handles(&session.visualization.handles, shown.pose, &ray, &pixel)
            }
            (None, _) => {
                picking::references(&shown.world, shown.pose, &shown.visibility, &ray, &[])
            }
        };
        self.viewport.candidate = 0;
        self.viewport.candidates = candidates;
    }

    /// Confirms the current candidate: picks it for the field being
    /// picked, or selects it (clicking nothing clears the selection); with
    /// `add`, toggles it in the selection set.
    fn confirm(&mut self, add: bool) -> anyhow::Result<()> {
        let hovered = self.viewport.hovered().cloned();
        // A click on nothing, for a step that is placed that way (a
        // dimension's text stands where it is clicked): the step is
        // done, and the next of its kind starts.
        if hovered.is_none()
            && let (Some(session), Some(at)) = (self.session(), self.viewport.pointer)
            && let Some(place) = &session.visualization.place
        {
            let pose = self.viewport.shown.pose;
            let ray = self
                .ray(at)
                .and_then(|ray| picking::place(&ray, pose.inverse()));
            if let Some(message) = ray.and_then(|ray| place(&ray)) {
                let kind = session.op.kind();
                self.update(message)?;
                self.update(Message::FinishEdit)?;
                self.history.seal();
                let next = crate::view::Ribbon::new(self)
                    .tools
                    .into_iter()
                    .map(|tool| tool.button.click())
                    .find(|m| matches!(m, Message::AddOperation { op, .. } if op.kind() == kind));
                if let Some(next) = next {
                    self.update(next)?;
                    self.edit_again = true;
                }
            }
            return Ok(());
        }
        if let (Some(field), Some(session)) = (self.picking.clone(), self.session()) {
            let picked = self.picked();
            // A joint pointed at, for a field that picks one.
            if let Some(Candidate::Joint { name, .. }) = &hovered {
                let Some(Widget::JointInput(input)) =
                    edit::widget_at(&session.visualization.dialog, &field)
                else {
                    return Ok(());
                };
                let message = input.set(name.clone());
                self.picking = None;
                self.viewport.candidates.clear();
                self.update(message)?;
                self.history.seal();
                self.pick_next();
                return Ok(());
            }
            let Some(Candidate::Reference { instance, source }) = hovered else {
                return Ok(());
            };
            // Of a placed part: as it is seen where the part is.
            let source = Source::placed(&instance, source);
            // A plane where a sketch is asked for: a sketch on it, drawn
            // first.
            let widget = edit::widget_at(&session.visualization.dialog, &field);
            let before = &self.viewport.shown.before;
            let sketch_wanted = match widget {
                Some(Widget::ReferenceInput(input)) => {
                    input.kind == ReferenceKind::Profile && !input.accepts(before, &source)
                }
                Some(Widget::ReferenceList(list)) => {
                    list.kinds.contains(&ReferenceKind::Profile) && !list.accepts(before, &source)
                }
                _ => false,
            };
            if sketch_wanted && ReferenceKind::Plane.accepts(before, &source) {
                return self.sketch_first(&field, source);
            }
            let message = match edit::widget_at(&session.visualization.dialog, &field) {
                // (Where on it the pointer was, for a field that asks.)
                Some(Widget::ReferenceInput(input)) => {
                    let at = picked.as_ref().filter(|p| p.instance.is_empty());
                    input.set_at(source, at.map(|p| p.at))
                }
                Some(Widget::ConnectorInput(input)) => match picked {
                    Some(picked) => input.set(picked),
                    None => return Ok(()),
                },
                // A box adds (or removes) the reference and keeps picking.
                Some(Widget::ReferenceList(list)) => {
                    let at = picked.as_ref().filter(|p| p.instance.is_empty());
                    let message = list.pick_at(source, at.map(|p| p.at));
                    self.viewport.candidates.clear();
                    self.update(message)?;
                    // (Whatever the reference brought, the box goes on
                    // picking.)
                    self.picking = Some(field);
                    self.history.seal();
                    return Ok(());
                }
                _ => return Ok(()),
            };
            self.picking = None;
            self.viewport.candidates.clear();
            self.update(message)?;
            self.history.seal();
            // On to the next reference to pick, if any.
            self.pick_next();
            if self.picking.is_some() {
                self.rebuild();
            }
            return Ok(());
        }
        if self.editing.is_empty() {
            let hovered = hovered.filter(|p| matches!(p, Candidate::Reference { .. }));
            match (hovered, add) {
                (Some(c), true) => {
                    if let Some(i) = self.selection.iter().position(|s| *s == c) {
                        self.selection.remove(i);
                    } else {
                        self.selection.push(c);
                    }
                }
                (Some(c), false) => self.selection = vec![c],
                (None, true) => {}
                (None, false) => self.selection.clear(),
            }
        }
        Ok(())
    }

    /// Makes a sketch on the plane `plane` before the edited step, puts
    /// it into the step's field `field` that asked for a sketch, and
    /// goes on to draw it: when it is done, the step's edit goes on.
    fn sketch_first(&mut self, field: &[usize], plane: Source) -> anyhow::Result<()> {
        let (Some((step, _)), Some(part)) = (self.editing.split_last(), self.focused_part()) else {
            return Ok(());
        };
        let step = step.clone();
        let id = self.workspace.part(&part)?.fresh_id("sketch");
        let sketch = workpiece_ops_sketch::Sketch::from_selection(
            &*id,
            &self.viewport.shown.before,
            &[plane],
        );
        let depth = self.history.depth();
        let back = Returning {
            editing: self.editing.clone(),
            checkpoint: self.edit_checkpoint,
            added: self.edit_added,
        };
        let at = self
            .workspace
            .part(&part)?
            .program
            .iter()
            .position(|op| op.id() == step)
            .context("the edited step is gone")?;
        self.change(None, |ws| {
            ws.part_mut(&part)?.program.insert(at, sketch.into());
            Ok(())
        })?;
        // (The scrubber after the step stays after it.)
        if let Some(s) = self.scrub.filter(|&s| at < s) {
            self.scrub = Some(s + 1);
        }
        // (Into the field, as it would be picked.)
        let made = Source::Profile(id.clone());
        let message = self.session().and_then(|session| {
            match edit::widget_at(&session.visualization.dialog, field)? {
                Widget::ReferenceInput(input) => Some(input.set(made)),
                Widget::ReferenceList(list) => Some(list.toggle(made)),
                _ => None,
            }
        });
        if let Some(message) = message {
            self.update(message)?;
        }
        // On to the sketch.
        self.picking = None;
        self.viewport.candidates.clear();
        self.edit_state = None;
        let mut editing = self.focus.clone();
        editing.push(id);
        self.editing = editing;
        self.edit_checkpoint = Some(depth);
        self.edit_untouched = true;
        self.edit_added = true;
        self.edit_keeps = false;
        self.returning = Some(back);
        self.history.seal();
        self.rebuild();
        Ok(())
    }

    /// Drags the grabbed handle by the pointer's move from `from` to `to`.
    fn drag(&mut self, from: PointerAt, to: PointerAt) -> anyhow::Result<()> {
        let Some(Candidate::Handle { name, part }) = self.viewport.dragging.clone() else {
            return Ok(());
        };
        let Some(session) = self.session() else {
            return Ok(());
        };
        // A canvas makes of a drag what it wants.
        if matches!(
            session.visualization.handles.get(&name),
            Some(Handle::Canvas(_) | Handle::Space(_))
        ) {
            self.canvas_event(CanvasEventKind::Drag, to, false)?;
            return Ok(());
        }
        let (Some(from), Some(to)) = (self.ray(from), self.ray(to)) else {
            return Ok(());
        };
        // Handles live in the focused part's coordinates.
        let local = self.viewport.shown.pose.inverse();
        let place = |r: &Ray| {
            Ray::new(
                local.transform_point(r.start()),
                local.transform_vector(r.direction()),
                r.projection(),
            )
        };
        let (Some(from), Some(to)) = (place(&from), place(&to)) else {
            return Ok(());
        };
        let message = match (session.visualization.handles.get(&name), part) {
            (Some(Handle::Linear(h)), None) => h.drag(&from, &to),
            (Some(Handle::Angular(h)), None) => h.drag(&from, &to),
            (Some(Handle::Planar(h)), _) => h.drag(&from, &to),
            (Some(Handle::Gizmo(g)), Some(GizmoPart::Scale)) => {
                let by = Gizmo::<Message>::factor(g.origin(), &from, &to);
                if let (Some(drag), Some(by)) = (&mut self.viewport.gizmo_drag, by) {
                    drag.factor *= by;
                }
                g.drag(GizmoPart::Scale, &from, &to)
            }
            (Some(Handle::Gizmo(g)), Some(part)) => match self.viewport.gizmo_drag {
                // From where the drag started, onto what it snaps to.
                Some(start) => {
                    let begun = self.ray(start.at).and_then(|r| place(&r));
                    begun
                        .and_then(|begun| Gizmo::<Message>::moved(start.pose, part, &begun, &to))
                        .map(|pose| {
                            let space = session.space().map(|(_, s)| s.targets.clone());
                            let pose = match space {
                                Some(targets) => {
                                    self.snap_gizmo(&targets, start.pose, part, pose, &to)
                                }
                                None => pose,
                            };
                            (g.on_drag)(pose)
                        })
                }
                None => g.drag(part, &from, &to),
            },
            _ => None,
        };
        match message {
            Some(message) => self.update(message),
            None => Ok(()),
        }
    }
}

impl Model {
    /// What snaps now: the settings', but nothing while snapping is held
    /// off.
    pub(crate) fn snap_kinds(&self) -> SnapKinds {
        let mut kinds = self.settings.snaps;
        kinds.on &= !self.snaps_held;
        kinds
    }

    /// What `ray` (in the focused part's coordinates) snaps to: of the
    /// world the edited step is applied to, and `own` (the step's own);
    /// `from` is where a drag started (for a perpendicular).
    fn snap_at(&self, own: &SnapTargets, ray: &Ray, from: Option<Point3>) -> Option<Snap> {
        let kinds = self.snap_kinds();
        if !kinds.on {
            return None;
        }
        let shown = &self.viewport.shown;
        let scene = crate::snap::scene(&shown.before, &shown.visibility);
        let pose = shown.pose;
        let pixel = |p: Point3| self.pixel(pose.transform_point(p));
        crate::snap::find(&[&scene, own], ray, &pixel, &kinds, from)
    }

    /// `pose`, where a drag of `part` of a gizmo that stood at `start`
    /// took it with the pointer's ray `ray`, snapped: the centre knob
    /// onto what the pointer snaps to, else onto the grid in the plane
    /// facing the eye; along an arrow, onto what the pointer snaps to
    /// as seen along the axis (where that is nearest it), else so far
    /// that it lands on the grid. What it snapped to shows.
    fn snap_gizmo(
        &mut self,
        own: &SnapTargets,
        start: Pose,
        part: GizmoPart,
        pose: Pose,
        ray: &Ray,
    ) -> Pose {
        let kinds = self.snap_kinds();
        let from = start.translation();
        let snap = self.snap_at(own, ray, Some(from));
        let at = pose.translation();
        let step = workpiece_core::handles::snap::grid_step(
            self.pixel(self.viewport.shown.pose.transform_point(at)),
        );
        let grid = kinds.takes(SnapKind::Grid);
        let (to, kind) = match part {
            GizmoPart::Free => match snap {
                Some(snap) => (snap.at, Some(snap.kind)),
                None if grid => (
                    workpiece_core::handles::snap::grid_in_plane(at, ray.direction(), step),
                    Some(SnapKind::Grid),
                ),
                None => (at, None),
            },
            GizmoPart::Translate(axis) => {
                let a = start.axis(axis);
                match snap {
                    Some(snap) => (from + a * (snap.at - from).dot(a), Some(snap.kind)),
                    None if grid => {
                        let travel = workpiece_core::handles::snap::grid_along(
                            from,
                            a,
                            (at - from).dot(a),
                            step,
                        );
                        (from + a * travel, Some(SnapKind::Grid))
                    }
                    None => (at, None),
                }
            }
            GizmoPart::Rotate(_) | GizmoPart::Scale => (at, None),
        };
        self.viewport.snapped = kind.map(|kind| Snap { at: to, kind });
        pose.with_translation(to)
    }
}

/// How far a joint's glyph reaches from its middle, in pixels: the
/// radius of its ring.
const JOINT_GLYPH: f64 = 14.0;

/// Whether what is called `name` in the world `before` a step is still
/// there in the world `after` it: a vertex at its place, an edge along
/// its curve, a face or a body at all.
fn still_there(before: &World, after: &World, name: &str) -> bool {
    use workpiece_core::world::topo::{EdgeId, VertexId};
    let (was, is) = (&before.topo, &after.topo);
    let size = |p: Point3| 1e-6 * (1.0 + p.length());
    if let Some(vertex) = was.resolve::<VertexId>(name).and_then(|v| was.vertex(v)) {
        let now = is.resolve::<VertexId>(name).and_then(|v| is.vertex(v));
        return now.is_some_and(|v| v.point.distance(vertex.point) <= size(vertex.point));
    }
    if was.resolve::<EdgeId>(name).is_some() {
        // (Its middle is still on it: an edge a step shortened is the
        // same edge, one that only shares its name is not.)
        let (Some(old), Some(new)) = (
            outline_of(before, name).into_iter().next(),
            is.resolve::<EdgeId>(name)
                .and_then(|_| outline_of(after, name).into_iter().next()),
        ) else {
            return false;
        };
        // (Halfway along it, by length.)
        let long: f64 = old.windows(2).map(|w| w[0].distance(w[1])).sum();
        let mut left = long / 2.0;
        let mut middle = match old.first() {
            Some(first) => *first,
            None => return false,
        };
        for w in old.windows(2) {
            let step = w[0].distance(w[1]);
            if step >= left {
                middle = w[0] + (w[1] - w[0]) * (left / step.max(1e-300));
                break;
            }
            left -= step;
            middle = w[1];
        }
        return new.windows(2).any(|w| {
            let along = w[1] - w[0];
            let t =
                ((middle - w[0]).dot(along) / along.length_squared().max(1e-300)).clamp(0.0, 1.0);
            (w[0] + along * t).distance(middle) <= size(middle).max(along.length() * 1e-3)
        });
    }
    was.referred(name).is_none() || is.referred(name).is_some()
}

/// The edge called `name` in `world` as points along it, or the edges
/// around the face called so; nothing of anything else.
fn outline_of(world: &World, name: &str) -> Vec<Vec<Point3>> {
    use workpiece_core::world::topo::{CoedgeKind, EdgeId, FaceId};
    let topo = &world.topo;
    let along = |edge: EdgeId| {
        let edge = topo.edge(edge)?;
        Some(topo.curve(edge.curve)?.polyline(edge.t0, edge.t1))
    };
    if let Some(edge) = topo.resolve::<EdgeId>(name) {
        return along(edge).into_iter().collect();
    }
    let Some(face) = topo.resolve::<FaceId>(name).and_then(|f| topo.face(f)) else {
        return Vec::new();
    };
    face.loops
        .iter()
        .filter_map(|&l| topo.loop_(l))
        .flat_map(|l| &l.coedges)
        .filter_map(|&c| match topo.coedge(c)?.kind {
            CoedgeKind::Edge { edge, .. } => along(edge),
            _ => None,
        })
        .collect()
}

/// The kinds a reference field takes, and planes with them where it
/// takes sketches: a sketch is drawn on a plane picked there.
fn with_planes(kinds: &[ReferenceKind]) -> Vec<ReferenceKind> {
    let mut all = kinds.to_vec();
    if all.contains(&ReferenceKind::Profile) && !all.contains(&ReferenceKind::Plane) {
        all.push(ReferenceKind::Plane);
    }
    all
}
