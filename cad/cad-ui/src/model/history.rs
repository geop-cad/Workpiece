use std::mem;

use shapecraft_cad_part::Workspace;

/// Undo and redo, as snapshots of the workspace. Parts are programs, not
/// built geometry, so a snapshot is small.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct History {
    /// The workspace before each change, oldest first.
    undo: Vec<Workspace>,
    /// The workspace after each undone change, most recently undone last.
    redo: Vec<Workspace>,
    /// What the last change edited, while further edits of the same thing
    /// continue it (a drag is many edits but one undo step).
    last_edit: Option<EditKey>,
}

/// An operation in a part, by part path and operation id.
pub(crate) type EditKey = (String, String);

impl History {
    /// Records a change from `before`. A change that continues the last
    /// edit of the same `key` joins its undo step. Clears the redo stack.
    pub(crate) fn record(&mut self, before: Workspace, key: Option<EditKey>) {
        if key.is_none() || key != self.last_edit {
            self.undo.push(before);
        }
        self.redo.clear();
        self.last_edit = key;
    }

    /// Ends the current edit, so the next one starts a new undo step.
    pub(crate) fn seal(&mut self) {
        self.last_edit = None;
    }

    /// Steps `workspace` back; false if there is nothing to undo.
    pub(crate) fn undo(&mut self, workspace: &mut Workspace) -> bool {
        self.seal();
        let Some(before) = self.undo.pop() else {
            return false;
        };
        self.redo.push(mem::replace(workspace, before));
        true
    }

    /// Steps `workspace` forward again; false if there is nothing to redo.
    pub(crate) fn redo(&mut self, workspace: &mut Workspace) -> bool {
        self.seal();
        let Some(after) = self.redo.pop() else {
            return false;
        };
        self.undo.push(mem::replace(workspace, after));
        true
    }

    /// How many changes there are to undo.
    pub(crate) fn depth(&self) -> usize {
        self.undo.len()
    }

    /// Undoes changes until `depth` are left, and forgets what they were:
    /// they cannot be redone.
    pub(crate) fn undo_to(&mut self, depth: usize, workspace: &mut Workspace) {
        while self.undo.len() > depth && self.undo(workspace) {}
        self.redo.clear();
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
