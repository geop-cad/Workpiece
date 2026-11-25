//! The geometry that operations on bodies share, and nothing of the
//! operations themselves.
//!
//! * [`sweep`] turns the areas of a sketch into solids along a line or
//!   around an axis; [`swept`] sweeps outlines along any path and lofts
//!   through sections; [`paths`] reads a path out of a sketch or a curve.
//! * [`blend`] (with [`roll`]) rounds edges and cuts them off.
//! * [`thick`] offsets faces: thickening, shelling, drafting, moving;
//!   [`heal`] takes faces off and closes the body over.
//! * [`surface`] works on sheets; [`split`] cuts by a plane; [`thread`]
//!   finds cylinders and cuts threads.
//! * [`known`] tells planes, cylinders and cones whatever surface
//!   they are made of.
//! * [`bodies`] puts a tool into the bodies there are, and [`ui`] holds
//!   the dialog fields operations on sketches share.

pub mod arcs;
pub mod blend;
pub mod bodies;
pub mod cut;
pub mod fill;
pub mod gn;
pub mod heal;
pub mod interpolate;
pub mod known;
mod local;
pub mod on;
pub mod paths;
pub mod roll;
pub mod split;
pub mod surface;
pub mod sweep;
pub mod swept;
pub mod thick;
pub mod thread;
mod tweak;
pub mod ui;

pub use blend::{
    By, Section, Shape as BlendShape, blend_chains, blend_each, blend_faces, blend_set_back,
    blend_solid, chain, full_round,
};
pub use bodies::Combine;
pub use split::{halves, halves_by, section};
pub use surface::{between, blended_sheet, faces_of, filled, rounded, ruled, trimmed, whole};
pub use thick::{
    drafted, faces_moved, faces_offset, faces_placed, faces_to_plane, thicken, thickened,
};
pub use thread::threaded;

#[cfg(test)]
mod tests;
