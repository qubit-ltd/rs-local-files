// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Value-only intersection of optional resource ceilings.

/// Returns the stricter finite limit, treating `None` as unbounded.
///
/// `Some(0)` remains a finite limit; validation belongs to the operation.
///
/// # Type Parameters
///
/// * `T` - The ordered resource-limit value.
///
/// # Parameters
///
/// * `requested` - The limit requested for the operation, or `None` if
///   unbounded.
/// * `ceiling` - The maximum permitted limit, or `None` if no ceiling applies.
///
/// # Returns
///
/// The smaller finite limit when both inputs are `Some`, the finite input when
/// only one is `Some`, or `None` when both inputs are unbounded.
#[inline]
pub(super) fn tighter<T: Ord>(requested: Option<T>, ceiling: Option<T>) -> Option<T> {
    match (requested, ceiling) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::tighter;

    /// Optional intersection must preserve finite zero and both absent sides.
    #[test]
    fn test_tighter_preserves_finite_limits() {
        assert_eq!(tighter::<usize>(None, None), None);
        assert_eq!(tighter(Some(3), None), Some(3));
        assert_eq!(tighter(None, Some(3)), Some(3));
        assert_eq!(tighter(Some(3), Some(5)), Some(3));
        assert_eq!(tighter(Some(5), Some(0)), Some(0));
    }
}
