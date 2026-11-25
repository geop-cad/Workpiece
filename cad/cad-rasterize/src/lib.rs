//! Rasterizing: built worlds and edit handles become flat buffers of
//! triangles, lines and vertices (point markers), ready to draw and to
//! pick from.
//!
//! * [`buffers`]: the primitives, and the [`Scene`] that holds them with
//!   what each one shows ([`Pick`]).
//! * [`world`] and [`handles`]: what a world (faces, edges, vertices,
//!   datums, instances) and an edit's handles look like.
//! * [`tessellate`]: faces sampled into triangles.
//! * [`quads`]: the buffers as GPU vertex data. Lines and vertices become
//!   quads whose corners the vertex shader pushes out in screen space, so
//!   they face the camera at a fixed width in pixels.
//! * [`palette`]: the colors.

pub mod buffers;
pub mod drawing;
pub mod handles;
pub mod palette;
pub mod quads;
pub mod tessellate;
pub mod world;

pub use buffers::{
    Color, Front, Label, Line, LineBuffer, Pick, PickId, Scene, Triangle, TriangleBuffer, Vertex,
    VertexBuffer,
};
pub use drawing::{Drawing, Placed, Scenes};
pub use world::Style;
