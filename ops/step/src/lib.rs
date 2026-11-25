//! Importing STEP files (ISO 10303-21 exchange structures of AP203,
//! AP214 and AP242 boundary representations), and writing them
//! ([`export`]: AP214, every curve and surface a B-spline).
//!
//! * [`part21`] reads a file's instances: one pass that only finds
//!   them, and arguments parsed when asked for, so that what a boundary
//!   representation does not use is never parsed.
//! * `geometry` turns the file's curves and surfaces into NURBS.
//! * [`import`] builds each solid: every face as a region of a patch of
//!   its surface, with its edges' curves there.
//! * [`ImportStep`] is the operation that adds a file's solids to a
//!   part.
//!
//! What is read: solids (`MANIFOLD_SOLID_BREP`, `BREP_WITH_VOIDS`) of
//! faces on planes, cylinders, cones, spheres, tori, surfaces of
//! revolution and of linear extrusion and B-spline surfaces, bounded by
//! lines, circles, ellipses and B-spline curves; in any length unit. A
//! face may go all around its surface, both ways around a ring's. A
//! file with no exact solid has its tessellated ones read (AP242: the
//! solids their triangles bound).
//! Not read yet: sheets and wires, and a face all around a surface that
//! has none of its seams as an edge the other way (a band around a
//! B-spline ring's tube). What is not read is refused, with the reason;
//! nothing is guessed.

mod assembly;
mod brep;
mod export;
mod geometry;
mod iges;
mod mesh;
mod operation;
pub mod part21;

#[cfg(test)]
mod tests;

pub use brep::{Body, Imported, import, import_sheets};
pub use export::export;
pub use iges::{IgesImported, export_iges, import_iges, is_iges};
pub use mesh::{import_3mf, import_mesh, import_stl, is_mesh, mended};
pub use operation::{ImportStep, ImportStepArgs};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    r.kind::<ImportStep>("import");
    r.icon("import", include_str!("../icons/import.svg"));
}
