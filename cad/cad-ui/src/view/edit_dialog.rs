use workpiece_core::widgets::Button;
use workpiece_core::{Hotkey, Key, Placement, Widget};

use super::Icon;
use crate::{Message, Model};

/// The popup that edits a step: its fields, and buttons to close it.
pub struct EditDialog {
    /// What the step is.
    pub icon: Icon,
    /// The step's id.
    pub title: String,
    /// The step's dialog and handles, as the model keeps them while it
    /// stays as it is: see [`Self::fields`] and [`Self::inspector`].
    session: std::rc::Rc<crate::edit::Session>,
    /// Where the fields are shown: in a popup, or in the ribbon in place
    /// of the tools.
    pub placement: Placement,
    /// The reference field being picked, by widget path.
    pub picking: Option<Vec<usize>>,
    /// What the view shows of a row of the dialog under the pointer.
    pub pointed: Option<workpiece_core::widgets::Pointed>,
    /// Why the step cannot be applied, if it cannot.
    pub error: Option<String>,
    /// Closes the dialog, keeping the changes.
    pub finish: Button<Message>,
    /// Closes the dialog, undoing the changes.
    pub cancel: Button<Message>,
    /// Whether the edited step shows applied as it changes.
    pub preview: bool,
    /// Turns the preview on or off.
    pub toggle_preview: Button<Message>,
}

impl EditDialog {
    /// The step's fields; their messages edit it.
    pub fn fields(&self) -> &Widget<Message> {
        &self.session.visualization.dialog
    }

    /// What the step's context menu offers now.
    pub fn menu(&self) -> &[workpiece_core::widgets::ToolButton<Message>] {
        &self.session.visualization.menu
    }

    /// For fields in the ribbon: those of what is selected in the step,
    /// for the panel at the side (see
    /// [`workpiece_core::EditVisualization::inspector`]).
    pub fn inspector(&self) -> Option<&Widget<Message>> {
        self.session.visualization.inspector.as_ref()
    }

    /// The dialog for the step `model` edits, if any.
    pub fn new(model: &Model) -> Option<Self> {
        let session = model.session()?;
        Some(Self {
            icon: Icon::of(&session.op),
            title: session.op.id().to_owned(),
            placement: session.visualization.placement,
            picking: model.picking.clone(),
            pointed: model.pointed.clone(),
            error: model.error(),
            finish: Button::new("Done", || Message::FinishEdit)
                .with_hotkey(Hotkey::named(Key::Enter)),
            // In a ribbon Escape puts the tool down; cancelling all the
            // work there takes more.
            cancel: Button::new("Cancel", || Message::CancelEdit).with_hotkey(
                match session.visualization.placement {
                    Placement::Popup => Hotkey::named(Key::Escape),
                    Placement::Ribbon => Hotkey::shift_named(Key::Escape),
                },
            ),
            session,
            preview: !model.preview_disabled,
            toggle_preview: {
                let on = model.preview_disabled;
                Button::new("Preview", move || Message::SetPreview(on))
                    .with_hotkey(Hotkey::shift('V'))
            },
        })
    }
}
