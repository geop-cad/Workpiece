//! Edit widgets: what an [`crate::Operation`] shows while its arguments are
//! being edited.
//!
//! Widgets do not change anything themselves. Each widget that edits holds
//! a [`WidgetHandler`] that turns the user's input into a *message* of type
//! `M`; the UI hands the message to whoever owns the state, which applies
//! it (and can record it, for undo) and rebuilds the widgets.
//!
//! An operation's widgets produce the operation's new arguments.
//! [`Widget::map`] lifts a widget to a bigger message type, so the
//! application can wrap them (say, into "replace this operation") and
//! every widget it shows produces the same message type, checked by the
//! compiler. 3D handles live apart from the dialog, in [`crate::handles`].

mod button;
mod check;
mod choice;
mod colour_input;
mod connector_input;
mod file_input;
mod hbox;
mod joint_input;
mod label_icon;
mod numeric_input;
mod pose_input;
mod reference_input;
mod reference_list;
mod table;
mod tool_button;
mod vbox;

use std::rc::Rc;

pub use button::Button;
pub use check::Check;
pub use choice::{Choice, Shown};
pub use colour_input::ColourInput;
pub use connector_input::{ConnectorInput, Picked, Scope};
pub use file_input::FileInput;
pub use hbox::HBox;
pub use joint_input::JointInput;
pub use label_icon::{kind_icon, label_icon, register_icons};
pub use numeric_input::NumericInput;
pub use pose_input::PoseInput;
pub use reference_input::ReferenceInput;
pub use reference_list::ReferenceList;
pub use table::{Pointed, Table};
pub use tool_button::{Group, Label, Mood, TextInput, ToolButton};
pub use vbox::VBox;

/// A callback that turns a `V` the user entered into a message `M`.
pub type WidgetHandler<V, M> = Box<dyn Fn(V) -> M>;

/// A message conversion, shared by every handler of a mapped widget.
pub type MapFn<M, N> = Rc<dyn Fn(M) -> N>;

/// `handler`, followed by `f`.
pub(crate) fn map_handler<V: 'static, M: 'static, N: 'static>(
    handler: WidgetHandler<V, M>,
    f: &MapFn<M, N>,
) -> WidgetHandler<V, N> {
    let f = f.clone();
    Box::new(move |v| f(handler(v)))
}

/// Any widget, producing messages of type `M`.
pub enum Widget<M> {
    /// A push button.
    Button(Button<M>),
    /// A choice of one of several options.
    Choice(Choice<M>),
    /// A number field.
    NumericInput(NumericInput<M>),
    /// Fields for a pose: position and Euler angles.
    PoseInput(PoseInput<M>),
    /// A field that picks a reference.
    ReferenceInput(ReferenceInput<M>),
    /// A field that picks a connector on a placed part.
    ConnectorInput(ConnectorInput<M>),
    /// A field that picks a joint of the assembly.
    JointInput(JointInput<M>),
    /// A box of references of several kinds.
    ReferenceList(ReferenceList<M>),
    /// A field that chooses a file.
    FileInput(FileInput<M>),
    /// Children stacked top to bottom.
    VBox(VBox<M>),
    /// Children side by side.
    HBox(HBox<M>),
    /// An icon button of a ribbon or a menu.
    ToolButton(ToolButton<M>),
    /// A labelled section of a ribbon.
    Group(Group<M>),
    /// A line of text.
    Label(Label),
    /// A text field.
    TextInput(TextInput<M>),
    /// A field that picks a colour.
    ColourInput(ColourInput<M>),
    /// A check mark.
    Check(Check<M>),
    /// Rows of like things under column headings.
    Table(Table<M>),
}

impl<M: 'static> Widget<M> {
    /// The same widget, its messages converted by `f`.
    pub fn map<N: 'static>(self, f: impl Fn(M) -> N + 'static) -> Widget<N> {
        self.map_rc(&(Rc::new(f) as MapFn<M, N>))
    }

    /// [`map`](Self::map) with a shared conversion, for mapping trees.
    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> Widget<N> {
        match self {
            Widget::Button(w) => Widget::Button(w.map_rc(f)),
            Widget::Choice(w) => Widget::Choice(w.map_rc(f)),
            Widget::NumericInput(w) => Widget::NumericInput(w.map_rc(f)),
            Widget::PoseInput(w) => Widget::PoseInput(w.map_rc(f)),
            Widget::ReferenceInput(w) => Widget::ReferenceInput(w.map_rc(f)),
            Widget::ConnectorInput(w) => Widget::ConnectorInput(w.map_rc(f)),
            Widget::JointInput(w) => Widget::JointInput(w.map_rc(f)),
            Widget::ReferenceList(w) => Widget::ReferenceList(w.map_rc(f)),
            Widget::FileInput(w) => Widget::FileInput(w.map_rc(f)),
            Widget::VBox(w) => Widget::VBox(w.map_rc(f)),
            Widget::HBox(w) => Widget::HBox(w.map_rc(f)),
            Widget::ToolButton(w) => Widget::ToolButton(w.map_rc(f)),
            Widget::Group(w) => Widget::Group(w.map_rc(f)),
            Widget::Label(w) => Widget::Label(w),
            Widget::TextInput(w) => Widget::TextInput(w.map_rc(f)),
            Widget::ColourInput(w) => Widget::ColourInput(w.map_rc(f)),
            Widget::Check(w) => Widget::Check(w.map_rc(f)),
            Widget::Table(w) => Widget::Table(w.map_rc(f)),
        }
    }

    /// The widgets inside this one, for a box, a group or a table (its
    /// rows).
    pub fn children(&self) -> &[Widget<M>] {
        match self {
            Widget::VBox(b) => &b.children,
            Widget::HBox(b) => &b.children,
            Widget::Group(g) => &g.children,
            Widget::Table(t) => &t.rows,
            _ => &[],
        }
    }

    /// The widget at `path`: child indices from this one down.
    pub fn at(&self, path: &[usize]) -> Option<&Widget<M>> {
        match path.split_first() {
            None => Some(self),
            Some((&first, rest)) => self.children().get(first)?.at(rest),
        }
    }

    /// Calls `f` with this widget and every widget inside it.
    pub fn visit<'a>(&'a self, f: &mut dyn FnMut(&'a Widget<M>)) {
        f(self);
        for child in self.children() {
            child.visit(f);
        }
    }
}

macro_rules! into_widget {
    ($($ty:ident),*) => {$(
        impl<M> From<$ty<M>> for Widget<M> {
            fn from(w: $ty<M>) -> Self {
                Widget::$ty(w)
            }
        }
    )*};
}

into_widget!(
    Button,
    Choice,
    NumericInput,
    PoseInput,
    ReferenceInput,
    ConnectorInput,
    JointInput,
    ReferenceList,
    FileInput,
    VBox,
    HBox,
    ToolButton,
    Group,
    TextInput,
    ColourInput,
    Check,
    Table
);

impl<M> From<Label> for Widget<M> {
    fn from(w: Label) -> Self {
        Widget::Label(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_converts_messages_through_the_tree() -> anyhow::Result<()> {
        let inner: Widget<f64> = VBox::new(vec![
            NumericInput::new("x", 1.0, |x| x * 2.0).into(),
            Button::new("reset", || 0.0).into(),
        ])
        .into();
        let Widget::VBox(outer) = inner.map(|x: f64| format!("{x}")) else {
            anyhow::bail!("expected a vbox");
        };
        let [Widget::NumericInput(field), Widget::Button(button)] = &outer.children[..] else {
            anyhow::bail!("expected a field and a button");
        };
        assert_eq!(field.set(3.0).as_deref(), Some("6"));
        assert_eq!(button.click(), "0");
        Ok(())
    }
}
