//! NURBS curves and surfaces: the only geometry the topology carries.
//!
//! Lines, circles, planes, cylinders, ... are all stored as (rational)
//! B-splines. Values are validated on construction and immutable after.
//!
//! * [`error`]: [`NurbsError`], why data was rejected.
//! * `validate`: the shared knot-vector and weight checks.
//! * `basis`: B-spline basis evaluation (knot span search, basis functions).
//! * [`point`]: [`ControlPoint`], what a control point type supports.
//! * [`curve`]: [`NurbsCurve`] over 3D or 2D points ([`Curve3`], [`Curve2`]).
//! * [`recognize`]: telling what a plane curve is (a line, an arc, a circle)
//!   within a tolerance.
//! * [`surface`]: [`NurbsSurface`], tensor-product surfaces in model space.

mod basis;
pub mod curve;
pub mod error;
pub mod point;
pub mod recognize;
pub mod surface;
mod validate;

pub use curve::{Curve2, Curve3, NurbsCurve, SEGMENTS_PER_SPAN};
pub use error::NurbsError;
pub use point::ControlPoint;
pub use recognize::{Recognized, recognize};
pub use surface::{AlongU, NurbsSurface};
