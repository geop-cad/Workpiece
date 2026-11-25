//! The application: its whole state, the [`Model`]; what is shown for it,
//! the [`View`], which is derived from the model; and the [`Message`]s the
//! view produces to change the model.
//!
//! Picking works the same way everywhere: moving the pointer finds the
//! *candidates* under it (nearest first) and stores them in the model's
//! viewport; the current one is highlighted, the right mouse button
//! cycles through them, and a left click confirms it. What counts as a
//! candidate depends on what the user is doing: while a step is edited,
//! its handles, or what the reference field being picked takes; otherwise
//! anything a reference can point at.

pub mod cam;
mod edit;
mod message;
pub mod model;
pub mod picking;
pub mod preview;
pub mod view;

pub use message::{CameraMove, Message, Panel, Pointer, Tab};
pub use model::Model;
pub use picking::Candidate;
pub use view::View;

#[cfg(test)]
mod tests;
