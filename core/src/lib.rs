//! The modelling core.
//!
//! * [`world`]: everything a part is made of — topology, datums, profiles,
//!   and instances of other parts.
//! * [`reference`](mod@reference): stable references into the world, and their resolution.
//! * [`operation`]: the trait every modelling operation implements.
//! * [`builder`]: what operations are applied to.
//! * [`widgets`] and [`handles`]: what an operation shows while its
//!   arguments are edited; handles are dragged with pick [`ray`]s.
//! * [`hotkey`]: the keys that press buttons.
//! * [`measure`]: the numbers of arguments, plain or formulas over the
//!   part's parameters.
//! * [`pose`]: rigid placements as dual quaternions.
//! * [`files`]: the files the program reads and writes, on disk or (in a
//!   browser) in memory.

extern crate self as shapecraft_core;

pub mod arguments;
pub mod builder;
pub mod cancel;
pub mod files;
pub mod handles;
pub mod hotkey;
pub mod math;
pub mod measure;
pub mod operation;
pub mod pose;
pub mod ray;
pub mod reference;
pub mod registry;
pub mod unit;
pub mod widgets;
pub mod world;

pub use arguments::{Arguments, Choices};
pub use builder::{Builder, Cache, Read, Standalone, Trace};
pub use cancel::{Cancel, Cancelled, Stopped};
pub use hotkey::{Hotkey, Key};
pub use math::Axis;
pub use measure::{Measure, Parameters};
pub use operation::{
    Edit, EditContext, EditVisualization, Kept, NoParts, Operation, Parts, Placement,
};
pub use operation::{Made, Shape};
pub use pose::Pose;
pub use ray::{Projection, Ray};
pub use reference::{
    Anything, EdgesOf, FaceOrBody, Position, Reference, ReferenceKind, Refers, Resolve,
    ResolveError, SketchOrBody, Sketched, Source,
};
pub use registry::{AnyEdit, AnyEditState, AnyOperation, IconOf, Registry, Tool, ToolContext};
pub use shapecraft_derive::Arguments;
pub use unit::Unit;
pub use widgets::{Widget, WidgetHandler};
pub use world::World;

/// Whether work can be spread over threads: everywhere but in a browser,
/// where a build runs on the one thread there is (between frames).
pub const HAS_THREADS: bool = !cfg!(target_arch = "wasm32");
