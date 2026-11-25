//! Errors raised when NURBS data is rejected.

/// Why NURBS data was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NurbsError {
    /// Degree must be at least 1.
    #[error("degree must be at least 1")]
    ZeroDegree,
    /// Fewer than `degree + 1` control points (in some direction).
    #[error("need at least degree + 1 control points")]
    TooFewControlPoints,
    /// `control_points.len()` differs from `count_u * count_v`.
    #[error("surface needs count_u * count_v control points")]
    ControlGridMismatch,
    /// `knots.len() != control_points + degree + 1` (in some direction).
    #[error("knot count must be control points + degree + 1")]
    KnotCountMismatch,
    /// Knots are not finite or not non-decreasing.
    #[error("knots must be finite and non-decreasing")]
    BadKnots,
    /// The parameter domain `[knots[p], knots[n]]` is empty.
    #[error("parameter domain is empty")]
    EmptyDomain,
    /// `weights.len()` differs from the number of control points.
    #[error("one weight per control point is required")]
    WeightCountMismatch,
    /// A weight is not finite or not strictly positive.
    #[error("weights must be finite and positive")]
    BadWeight,
    /// A control point coordinate is not finite.
    #[error("control points must be finite")]
    NonFinitePoint,
}
