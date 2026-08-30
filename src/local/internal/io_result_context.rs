// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Context normalization for descriptor errors unavailable to public fixtures.
// qubit-style: allow source-test-pair
// qubit-style: allow inline-tests
// qubit-style: allow explicit-imports
// Live descriptors cannot be invalidated through the public API.

use std::io::Result;
use std::path::Path;

use super::path_operations::add_path_context;

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
pub(crate) fn with_path_context<T>(result: Result<T>, operation: &'static str, path: &Path) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => Err(add_path_context(error, operation, path)),
    }
}

// These tests pin the private error-context adapter's exact source/path
// propagation. The public API only returns the fully assembled error and
// cannot isolate this adapter; exposing a hook would be an undesirable test
// seam. Public error integration tests cover the observable result.
#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::with_path_context;

    #[test]
    fn test_with_path_context_preserves_success_values() {
        assert_eq!(
            with_path_context(Ok(7_u8), "read", Path::new("a")).expect("success should remain successful"),
            7
        );
    }

    #[test]
    fn test_with_path_context_enriches_errors_with_operation_and_path() {
        let error = with_path_context::<()>(
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing")),
            "read",
            Path::new("a"),
        )
        .expect_err("error should remain an error");
        assert!(error.to_string().contains("failed to read 'a'"));
        assert!(error.to_string().contains("missing"));
    }
}
