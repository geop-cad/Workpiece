use shapecraft_cad_part::{AnyOperation, Part, Workspace};
use shapecraft_core::Source;

use shapecraft_core::{Hotkey, Key};

use super::Icon;
use crate::Model;

/// Removes the step selected in the timeline.
pub const REMOVE: Hotkey = Hotkey::named(Key::Delete);

/// One step of a program in the timeline's tree.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramStep {
    /// Its step path from the active part.
    pub path: Vec<String>,
    /// Its position in its program.
    pub index: usize,
    /// What it is.
    pub icon: Icon,
    /// Its id.
    pub title: String,
    /// What it does, in a few words.
    pub description: String,
    /// The steps of the part it places, for a tree node: filled while
    /// expanded. `None` for a step that places nothing.
    pub children: Option<Vec<ProgramStep>>,
    /// Whether its children show.
    pub expanded: bool,
    /// Whether it is the step being edited.
    pub editing: bool,
    /// Whether it is the step selected.
    pub selected: bool,
    /// Whether it is in the focused part's program (where the scrubber
    /// runs).
    pub in_focus: bool,
    /// Whether it places the focused part.
    pub focused: bool,
    /// Whether it is in the focused part's program after the scrubber or
    /// the edited step, so not shown.
    pub rolled_back: bool,
}

/// The scrubber: how many steps of the focused part are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scrubber {
    /// Steps shown.
    pub at: usize,
    /// Steps there are.
    pub of: usize,
}

/// The active part's program as a tree: steps that place a part open into
/// that part's steps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Timeline {
    /// The active part's path.
    pub part: Option<String>,
    /// The focus, as step path, with the id of each placed part's step.
    pub focus: Vec<String>,
    /// The steps, in order.
    pub steps: Vec<ProgramStep>,
    /// The scrubber in the focused part's program.
    pub scrubber: Option<Scrubber>,
    /// The selected step, while it can be removed with [`REMOVE`]: no
    /// step is edited.
    pub remove: Option<Vec<String>>,
}

/// A few words on `source`.
pub fn describe_source(source: &Source) -> String {
    source.described()
}

/// What `op` does, in a few words: what it says itself.
fn describe(op: &AnyOperation) -> String {
    op.summary()
}

/// Builds the steps of one program.
struct Tree<'a> {
    workspace: &'a Workspace,
    model: &'a Model,
    /// The steps shown of the focused part.
    cut: usize,
}

impl Tree<'_> {
    fn steps(&self, part: &Part, prefix: &[String]) -> Vec<ProgramStep> {
        let in_focus = prefix == self.model.focus.as_slice();
        part.program
            .iter()
            .enumerate()
            .map(|(index, op)| {
                let path: Vec<String> =
                    prefix.iter().cloned().chain([op.id().to_owned()]).collect();
                let expanded = self.model.expanded.contains(&path);
                // A placed part is a tree node, filled while expanded.
                let children = op
                    .placed()
                    .map(|(placed, _)| match self.workspace.part(placed) {
                        Ok(placed) if expanded => self.steps(placed, &path),
                        _ => Vec::new(),
                    });
                ProgramStep {
                    index,
                    icon: Icon::of(op),
                    title: op.id().to_owned(),
                    description: describe(op),
                    children,
                    expanded,
                    editing: self.model.editing == path,
                    selected: self.model.step.as_ref() == Some(&path),
                    in_focus,
                    focused: !self.model.focus.is_empty() && self.model.focus == path,
                    rolled_back: in_focus && index >= self.cut,
                    path,
                }
            })
            .collect()
    }
}

impl Timeline {
    /// The active part's tree in `model`.
    pub fn new(model: &Model) -> Self {
        let Some(part) = model.active_part() else {
            return Self::default();
        };
        let focused = model
            .focused_part()
            .and_then(|p| model.workspace.part(&p).ok());
        let of = focused.map_or(0, |p| p.program.len());
        let edited = model
            .editing
            .last()
            .and_then(|id| focused?.program.iter().position(|op| op.id() == id));
        let at = edited.map_or_else(|| model.scrub.unwrap_or(of).min(of), |i| i + 1);
        let tree = Tree {
            workspace: &model.workspace,
            model,
            cut: at,
        };
        Self {
            part: model.active_part.clone(),
            focus: model.focus.clone(),
            steps: tree.steps(part, &[]),
            scrubber: focused.map(|_| Scrubber { at, of }),
            remove: model.step.clone().filter(|_| model.editing.is_empty()),
        }
    }
}
