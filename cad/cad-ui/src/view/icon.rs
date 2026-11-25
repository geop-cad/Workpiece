/// An icon the host draws next to a tool or a timeline step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    /// Undo.
    Undo,
    /// Redo.
    Redo,
    /// A new part, made in place.
    NewPart,
    /// An icon by its name: one a crate of operations told of (that of
    /// an operation, `"extrude"`, or of a tool of its own, `"line"`).
    Named(&'static str),
    /// Focusing a placed part.
    Focus,
    /// Finishing an edit.
    Done,
    /// Cancelling an edit.
    Cancel,
}

impl Icon {
    /// The icon of an operation.
    pub fn of(op: &workpiece_part::AnyOperation) -> Self {
        // (As its kind was told of by the crate that has it.)
        Icon::Named(op.icon())
    }
}
