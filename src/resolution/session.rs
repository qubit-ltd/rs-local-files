// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

pub(crate) use super::resolution_session::ResolutionSession;
use crate::error::LocalFileError;
use crate::error::LocalFileOperation;
use crate::error::LocalResourceKind;
use crate::error::LocalResourceLimitError;

/// Builds a resource-limit error for an unrepresentable observed value.
///
/// # Parameters
///
/// * `kind` - Resource category whose measurement overflowed.
///
/// # Returns
///
/// An error recording an overflow with no representable observation.
pub(crate) fn overflow_error(kind: LocalResourceKind) -> LocalFileError {
    limit_error(kind, usize::MAX, true)
}

/// Builds an error using the largest representable limit.
///
/// # Parameters
///
/// * `kind` - Resource category associated with the measurement.
/// * `observed` - Measured resource usage.
/// * `overflow` - Whether the measurement could not be represented.
///
/// # Returns
///
/// A local file error containing the resource-limit facts.
pub(super) fn limit_error(kind: LocalResourceKind, observed: usize, overflow: bool) -> LocalFileError {
    limit_error_with_limit(kind, usize::MAX, observed, overflow)
}
/// Builds a resource-limit error while preserving the supplied observation.
///
/// # Parameters
///
/// * `kind` - Resource category associated with the measurement.
/// * `limit` - Maximum permitted resource usage.
/// * `observed` - Measured resource usage.
/// * `overflow` - Whether the measurement could not be represented.
///
/// # Returns
///
/// A local file error containing the resource-limit facts.
pub(super) fn limit_error_with_limit(
    kind: LocalResourceKind,
    limit: usize,
    observed: usize,
    overflow: bool,
) -> LocalFileError {
    let error = LocalResourceLimitError::new(kind, limit, limit.saturating_sub(observed.min(limit)), 1);
    let error = if overflow {
        error.with_overflow()
    } else {
        error.with_observed(observed)
    };
    LocalFileError::from_resource_limit(LocalFileOperation::Metadata, None, error)
}
