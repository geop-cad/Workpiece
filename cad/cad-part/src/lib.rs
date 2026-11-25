//! Parts: a program of operations that, applied in order, builds a world;
//! and the [`Workspace`] of them, which builds parts by path.

mod any_operation;
mod effect;
pub mod engine;
mod material;
pub use material::{LIBRARY_FILE, Library, Material, Texture};
mod naming;
pub use naming::Repair;
mod part;
mod preview;
pub use preview::{Pictured, Preview};
mod properties;
pub use properties::{Kind, Property, SCHEMA_FILE, Schema};
pub mod pool;
mod workspace;

pub use any_operation::{AnyEdit, AnyEditState, AnyOperation, registry};
pub use engine::{Built, Cancel, Cancelled, Engine};
pub use part::{Parameter, Part, Variant};
pub use workspace::{Workspace, WorkspaceError, is_step};
