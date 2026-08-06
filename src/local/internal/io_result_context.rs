// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Context normalization for descriptor errors unavailable to public fixtures.
// qubit-style: allow source-test-pair
// Live descriptors cannot be invalidated through the public API.

use std::io::Result;
use std::path::Path;

use super::path_operations::add_path_context;

/// Coverage-only entry point for context normalization.
#[cfg(coverage)]
pub fn coverage_with_path_context<T>(
    result: Result<T>,
    operation: &'static str,
    path: &Path,
) -> Result<T> {
    with_path_context(result, operation, path)
}

/// Adds path context to an I/O result without a call-site closure.
///
/// # Type Parameters
///
/// * `T` - The successful value carried through unchanged.
///
/// # Parameters
///
/// * `result` - The I/O result to return or enrich with context.
/// * `operation` - The operation description added to an error.
/// * `path` - The path added to an error.
///
/// # Returns
///
/// The original success value, or the original error enriched with the
/// operation and path.
///
/// # Errors
///
/// Returns the supplied error with path context when `result` is `Err`.
pub(crate) fn with_path_context<T>(
    result: Result<T>,
    operation: &'static str,
    path: &Path,
) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => Err(add_path_context(error, operation, path)),
    }
}
