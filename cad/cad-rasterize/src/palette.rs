//! The viewport's colors, linear RGBA (the sRGB hex each comes from in
//! brackets).
//!
//! Neutral solids, warm ochre for reference geometry, dark walnut for
//! handles (so the two never blend), blue for hover and selection, and
//! muted x, y and z colors shared by frames and gizmos.

use crate::Color;

/// Solids (`#CFD2D8`).
pub const MODEL: Color = [0.62, 0.64, 0.68, 1.0];
/// Faces of sheet bodies: surfaces, not solids.
pub const SURFACE: Color = [0.42, 0.60, 0.74, 1.0];
/// Solid edges (`#595D64`).
pub const EDGE: Color = [0.10, 0.11, 0.13, 1.0];
/// The helix that shows a thread that is said of a face, not cut
/// into it (`#95999F`).
pub const THREAD: Color = [0.30, 0.32, 0.35, 1.0];
/// Vertices and frame origins (charcoal, `#2A2E35`).
pub const POINT: Color = [0.023, 0.027, 0.036, 1.0];
/// Datum axes and planes (ochre, `#D9A15C`); planes are filled
/// translucently.
pub const REFERENCE: Color = [0.694, 0.356, 0.107, 1.0];
/// Edit handles (walnut, `#7A4A2A`).
pub const HANDLE: Color = [0.195, 0.069, 0.023, 1.0];
/// A hovered or dragged handle (sky blue, `#64B4FF`).
pub const HANDLE_HOVER: Color = [0.127, 0.456, 1.0, 1.0];
/// Hovered model entities (light blue, `#6FA4F8`).
pub const HOVER: Color = [0.159, 0.371, 0.939, 1.0];
/// Selected model entities (blue, `#2F7DF6`).
pub const SELECTION: Color = [0.028, 0.205, 0.922, 1.0];

/// The x axis (brick red, `#C8504B`).
pub const X: Color = [0.578, 0.080, 0.070, 1.0];
/// The y axis (leaf green, `#5FA052`).
pub const Y: Color = [0.114, 0.352, 0.084, 1.0];
/// The z axis (steel blue, `#4A78C8`).
pub const Z: Color = [0.069, 0.188, 0.578, 1.0];
/// x, y and z, by [`Axis::index`](workpiece_core::Axis::index).
pub const AXES: [Color; 3] = [X, Y, Z];

/// `color` with opacity `alpha`.
#[must_use]
pub fn with_alpha([r, g, b, _]: Color, alpha: f32) -> Color {
    [r, g, b, alpha]
}

/// `color` as a ghost: what is shown around a subpart edited in context,
/// faded towards grey and mostly transparent.
#[must_use]
pub fn ghost([r, g, b, a]: Color) -> Color {
    const KEEP: f32 = 0.35;
    const GREY: f32 = 0.45;
    const OPACITY: f32 = 0.18;
    let fade = |c: f32| c * KEEP + GREY * (1.0 - KEEP);
    [fade(r), fade(g), fade(b), a * OPACITY]
}

/// Sketch curves that can still move (blue, `#2F6FE4`).
pub const SKETCH_FREE: Color = [0.028, 0.159, 0.776, 1.0];
/// Sketch curves that are fully constrained, and finished sketches
/// (near black, `#1F2328`).
pub const SKETCH: Color = [0.014, 0.017, 0.021, 1.0];
/// Construction geometry of a sketch (grey, `#8A919C`).
pub const SKETCH_CONSTRUCTION: Color = [0.254, 0.283, 0.332, 1.0];
/// Sketch geometry projected from the model (violet, `#9B4DCA`).
pub const SKETCH_PROJECTED: Color = [0.328, 0.074, 0.591, 1.0];
/// Constraints in conflict (red, `#C83C37`).
pub const CONFLICT: Color = [0.578, 0.045, 0.038, 1.0];
/// What a sketch tool is about to make (orange, `#E08A1E`).
pub const PREVIEW: Color = [0.745, 0.254, 0.013, 1.0];
/// Constraint badges (green, `#3C8C5A`).
pub const BADGE: Color = [0.045, 0.262, 0.102, 1.0];
