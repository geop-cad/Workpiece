//! Tolerance values themselves.

use super::ValidationError;
use crate::world::topo::EntityId;

/// A tolerance must be finite and non-negative.
pub(super) fn check(id: EntityId, tolerance: f64) -> Result<(), ValidationError> {
    if tolerance.is_finite() && tolerance >= 0.0 {
        Ok(())
    } else {
        Err(ValidationError::BadTolerance { id, tolerance })
    }
}
