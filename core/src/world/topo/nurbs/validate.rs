//! Validation shared by curves and surfaces.

use super::error::NurbsError;

/// Checks a degree / control-point count / knot vector triple.
pub(super) fn check_basis(degree: usize, n_points: usize, knots: &[f64]) -> Result<(), NurbsError> {
    if degree == 0 {
        return Err(NurbsError::ZeroDegree);
    }
    if n_points < degree + 1 {
        return Err(NurbsError::TooFewControlPoints);
    }
    if knots.len() != n_points + degree + 1 {
        return Err(NurbsError::KnotCountMismatch);
    }
    if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| w[0] > w[1]) {
        return Err(NurbsError::BadKnots);
    }
    if knots[degree] >= knots[n_points] {
        return Err(NurbsError::EmptyDomain);
    }
    Ok(())
}

/// Checks one weight per control point, each finite and positive.
pub(super) fn check_weights(weights: &[f64], n_points: usize) -> Result<(), NurbsError> {
    if weights.len() != n_points {
        return Err(NurbsError::WeightCountMismatch);
    }
    if weights.iter().any(|w| !w.is_finite() || *w <= 0.0) {
        return Err(NurbsError::BadWeight);
    }
    Ok(())
}
