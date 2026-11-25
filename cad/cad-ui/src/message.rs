use std::collections::BTreeMap;

use shapecraft_cad_part::{AnyEditState, AnyOperation, Parameter, Part, Variant};

/// A change to the [`Model`](crate::Model). Everything the user does
/// becomes one of these; [`Model::update`](crate::Model::update) applies
/// it, and records changes to the workspace for undo.
///
/// Steps are addressed by *step paths*: operation ids from the active
/// part down, where every id but the last is an
/// [`AddPart`](shapecraft_ops_instances::AddPart) and the next id is a step of
/// the part it places.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Make the part at this path the one being edited.
    Open(String),
    /// Parts read from files, by path: added to the workspace as saved
    /// (parts already there stay), then the part at `open`, if any, is
    /// opened.
    Load {
        /// The parts read.
        parts: BTreeMap<String, Part>,
        /// The part to open.
        open: Option<String>,
    },
    /// Add a new, empty part at this path (never saved) and open it.
    New(String),
    /// Close the active part, dropping it from the workspace unless
    /// another part places it.
    Close,
    /// The active part was written to its file as it is now.
    Saved(String),
    /// From now on, the active part is the file at this path (it is
    /// written there next).
    SaveAs(String),
    /// Show or hide a datum, body or sketch of the focused part by name,
    /// overriding whether it is hidden by default (datums that later
    /// steps use are).
    SetVisible {
        /// Its name.
        name: String,
        /// Whether to show it.
        visible: bool,
    },
    /// The focused part is made of this material from now on (`None`:
    /// of none), where its steps say nothing else.
    SetPartMaterial(Option<String>),
    /// Replace the materials of the library: what every part chooses
    /// from.
    SetLibrary {
        /// Its materials.
        materials: Vec<shapecraft_cad_part::Material>,
    },
    /// Show the panel (or, `None`, none): the materials, or the part's
    /// parameters.
    ShowPanel(Option<Panel>),
    /// The picture or the camera view that is edited is taken from
    /// where the view is now.
    TakeView,
    /// Send the camera to a named camera view of the part that shows.
    LookFrom(String),
    /// Add a drawing to the focused part and open it: made from this
    /// sheet template, or (`None`) an empty sheet.
    NewDrawing(Option<String>),
    /// Cut the model open along a line drawn in a view of the drawing
    /// that is edited: the part gets a sketch of the line and a section
    /// along it, and the sheet a view of that section.
    CutSection {
        /// The view the line is drawn in.
        view: String,
        /// The corners of the line, in that view's own coordinates.
        line: Vec<[f64; 2]>,
        /// Where the section view stands on the sheet.
        at: [f64; 2],
    },
    /// The section the section view `view` of the drawing that is
    /// edited is of shows the other side from now on: what it takes
    /// away is kept, and what it kept taken away — in the part's
    /// Section step, which the view follows.
    FlipSection {
        /// The section view.
        view: String,
    },
    /// The line a section view of the drawing that is edited cuts
    /// along is this from now on: the part's sketch of it is drawn
    /// anew.
    MoveSectionLine {
        /// The section view.
        view: String,
        /// The corners of the line, in the own coordinates of the view
        /// it shows in.
        line: Vec<[f64; 2]>,
        /// Whether that is it: a drag that goes on is one change.
        settled: bool,
    },
    /// The focused part says this of itself from now on (nothing: it
    /// says it no more).
    SetProperty {
        /// The property's name.
        name: String,
        /// What the part says.
        value: String,
    },
    /// Replace the properties the parts of the workspace have.
    SetSchema {
        /// The properties.
        properties: Vec<shapecraft_cad_part::Property>,
    },
    /// Replace the sheet templates of the workspace.
    SetTemplates {
        /// The templates.
        templates: Vec<shapecraft_ops_drawing::Template>,
    },
    /// Put something said of the model on the drawing that is edited.
    PlaceNote {
        /// The annotation's name in the model.
        note: String,
        /// The view it goes in; `None`: the first that sees it true.
        view: Option<String>,
        /// Where its text stands, in millimetres from the view; `None`:
        /// by itself, off what it is about.
        at: Option<[f64; 2]>,
    },
    /// Put a centre line or the trace of a datum on the drawing that
    /// is edited, in the view `view`.
    PlaceLine {
        /// The view.
        view: String,
        /// What it is of.
        of: shapecraft_ops_drawing::sheet::LineOf,
    },
    /// Show, on the drawing that is edited, what is said of the model
    /// and is not on the sheet yet as what it would be (`Some`): in
    /// the view named, or in every view that sees it true. `None`
    /// shows the sheet as it is.
    PreviewNotes(Option<Option<String>>),
    /// Stand the text of something said of the model elsewhere on it.
    MoveNote {
        /// The annotation's name in the model.
        name: String,
        /// Where its text stands now, as the model shows.
        at: [f64; 3],
        /// Whether that is it: a drag that goes on is one change.
        settled: bool,
    },
    /// The focused part's numbers are in this unit of length from now
    /// on (the numbers stay as they are).
    SetUnit(shapecraft_core::Unit),
    /// Display the model this way: cut open, its parts apart.
    SetDisplay(crate::model::display::Display),
    /// Show this body of the focused part alone, the other bodies
    /// hidden — or, if it is alone already, show them all again.
    Isolate(String),
    /// Append `op` to the program of the part at `part`. Added to the
    /// focused part, it is opened for editing.
    AddOperation {
        /// The part's path.
        part: String,
        /// The new operation; its id must be free in the part.
        op: AnyOperation,
    },
    /// Replace the operation with `op`'s id in the part at `part` by `op`:
    /// what an operation's edit dialog and handles produce.
    ReplaceOperation {
        /// The part's path.
        part: String,
        /// The edited operation.
        op: AnyOperation,
    },
    /// Make a new part and add it to the focused one, then work on it
    /// where it is: for building an assembly part by part. With no path,
    /// the host asks for one and sends this again with it.
    NewPartHere(Option<String>),
    /// Replace what a step of a part keeps in the part's cache: what it
    /// worked out, set by hand (where a placed part is).
    Keep {
        /// The part's path.
        part: String,
        /// The step's id.
        id: String,
        /// Its entries from now on.
        kept: shapecraft_core::Kept,
    },
    /// Move the step at this step path to position `to` of its program.
    /// Refused if that puts a step before one it uses.
    MoveOperation {
        /// The step's path.
        path: Vec<String>,
        /// Its new position.
        to: usize,
    },
    /// Stop the build that is running beside the interface: what was
    /// shown stays, and the part builds again when it is next changed
    /// (the step that takes too long removed, or made lighter).
    StopBuild,
    /// Remove the step at this step path from its program. Refused while
    /// another step uses what it makes.
    RemoveOperation(Vec<String>),
    /// Rename the step at this step path; the steps that use what it
    /// makes follow.
    RenameOperation {
        /// The step's path.
        path: Vec<String>,
        /// Its new name.
        name: String,
    },
    /// Select the step at this step path in the timeline, or none.
    SelectStep(Option<Vec<String>>),
    /// Show this tab of the ribbon.
    SetTab(Tab),
    /// Replace the parameters and the table of variants of the part at
    /// `part`.
    SetTable {
        /// The part's path.
        part: String,
        /// Its parameters, in order.
        parameters: Vec<Parameter>,
        /// Its variants.
        variants: Vec<Variant>,
    },
    /// Show the active part as this variant; `None` for the part as it is
    /// by default.
    ShowVariant(Option<String>),
    /// The edit session's new state (the tool in use, what is selected
    /// inside the edited operation): what an operation's dialog and
    /// handles produce besides edited operations. Neither saved nor
    /// undone.
    EditState(AnyEditState),
    /// Several changes at once, in order.
    Batch(Vec<Message>),
    /// Time passed, in seconds: moves the camera on, while it is on its
    /// way to a new view.
    Tick(f32),
    /// Undo the last change to the workspace.
    Undo,
    /// Redo the last undone change.
    Redo,
    /// The user finished one edit (a drag, a field): the next edit of the
    /// same operation is its own undo step.
    EndEdit,
    /// Open the step at this step path for editing, focusing the part it
    /// is in.
    Edit(Vec<String>),
    /// Close the edit dialog, keeping the changes.
    FinishEdit,
    /// Show the edited step applied as it changes (and what follows it as
    /// ghosts), or not: without, large parts stay quick to edit.
    SetPreview(bool),
    /// Whether the model shows cut at the plane of a sketch that is
    /// drawn on a plane (a setting).
    SetSketchSection(bool),
    /// Close the edit dialog, undoing every change since it opened.
    CancelEdit,
    /// Expand or collapse the tree node of the placed part at this step
    /// path.
    Toggle(Vec<String>),
    /// Show the placed part at this step path in color, the rest as
    /// ghosts; the empty path focuses the active part itself.
    Focus(Vec<String>),
    /// Show the focused part after only this many steps; `None` shows all.
    Scrub(Option<usize>),
    /// Start picking a reference for the edit dialog's reference field at
    /// this widget path (child indices from the dialog's root), or stop
    /// picking.
    Pick(Option<Vec<usize>>),
    /// Something happened to the pointer over the viewport.
    Pointer(Pointer),
    /// Move the camera.
    Camera(CameraMove),
    /// The viewport's size in pixels, so handles keep their size on
    /// screen.
    ViewportSize([f32; 2]),
    /// Open the context menu at this position in the viewport (where the
    /// right or middle button is held), or close it.
    ContextMenu(Option<[f32; 2]>),
    /// Empty the selection set.
    ClearSelection,
}

/// A panel that shows over the view while it is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    /// The materials of the library, to change.
    Materials,
    /// The part's parameters and its variants.
    Parameters,
    /// What the part says of itself, and the properties parts have.
    Properties,
    /// The sheet templates drawings start from.
    Templates,
    /// What a new drawing starts from: asked before it is made.
    NewDrawing,
}

/// A tab of the ribbon: the tools of one kind of work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tab {
    /// Solids: datums, sketches, extrusions, revolutions, booleans, and
    /// subdivision surfaces.
    #[default]
    Modeling,
    /// Surfaces: the same, making sheets, and polygon meshes.
    Surfacing,
    /// Assemblies: placing parts.
    Assembly,
    /// Annotations: datum features, dimensions and geometric tolerances
    /// said on the model, and the drawing made of them.
    Annotate,
    /// Properties: what the part is made of and how it looks, the
    /// materials, the part's parameters, and pictures of it.
    Properties,
    /// Sheet metal: plates, flanges bent up from them, and their flat
    /// patterns.
    SheetMetal,
}

impl Tab {
    /// Every tab, in order.
    pub const ALL: [Tab; 6] = [
        Tab::Modeling,
        Tab::Surfacing,
        Tab::Assembly,
        Tab::SheetMetal,
        Tab::Annotate,
        Tab::Properties,
    ];

    /// Its name.
    pub fn label(self) -> &'static str {
        match self {
            Tab::Modeling => "Modeling",
            Tab::Surfacing => "Surfacing",
            Tab::Assembly => "Assembly",
            Tab::SheetMetal => "Sheet metal",
            Tab::Annotate => "Annotate",
            Tab::Properties => "Properties",
        }
    }
}

/// Pointer input over the viewport. Positions are in pixels from the
/// viewport's top left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pointer {
    /// The pointer is at `position` in a viewport of `size`: hovering
    /// finds what is under it; while a handle is dragged, it drags it.
    Moved {
        /// The pointer position.
        position: [f32; 2],
        /// The viewport size.
        size: [f32; 2],
    },
    /// The pointer left the viewport.
    Left,
    /// Cycle to the next candidate under the pointer.
    Cycle,
    /// Confirm the current candidate (left click): pick it for the
    /// reference field being picked, or select it. With `add`, toggle it in
    /// the selection set instead of selecting only it.
    Click {
        /// Add to (or remove from) the selection set.
        add: bool,
    },
    /// A second click right after one, in the same place.
    DoubleClick,
    /// The left button went down: what is under the pointer now is what a
    /// drag starting from here grabs.
    Press,
    /// A left drag starts (the pointer has moved a little since the
    /// press): on a handle pressed, it grabs it.
    DragStart,
    /// The left drag ends.
    DragEnd,
}

/// A camera movement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraMove {
    /// Turn like a trackball by a pointer movement in pixels.
    Orbit([f32; 2]),
    /// Move the view with the pointer by a movement in pixels.
    Pan([f32; 2]),
    /// Zoom by a scroll amount (positive is closer).
    Zoom(f32),
    /// Look at the target from this direction (towards the eye).
    LookFrom([f32; 3]),
    /// Switch between orthographic and perspective projection.
    ToggleProjection,
    /// Back to the starting view.
    Home,
    /// Face the plane the edited step is edited in (a sketch's), square
    /// on.
    FacePlane,
}
