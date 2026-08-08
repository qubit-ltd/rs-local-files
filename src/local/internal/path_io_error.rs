// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Path-aware I/O error context.
// qubit-style: allow source-test-pair
// qubit-style: allow inline-tests
// qubit-style: allow explicit-imports
// Private behavior is covered through public integration tests.

use std::io::Error;
use std::path::{Path, PathBuf};

/// An I/O error annotated with the failed operation and path.
#[derive(Debug)]
pub struct PathIoError {
    /// Description of the filesystem operation that failed.
    operation: &'static str,
    /// Path involved in the failed operation.
    path: PathBuf,
    /// Native I/O error that caused the failure.
    source: Error,
}

impl PathIoError {
    /// Creates path-aware context around an I/O error.
    ///
    /// # Parameters
    /// - `operation`: Operation that failed.
    /// - `path`: Path involved in the operation.
    /// - `source`: Native I/O error.
    ///
    /// # Returns
    /// A contextual error retaining `source`.
    #[inline]
    pub(super) fn new(operation: &'static str, path: &Path, source: Error) -> Self {
        Self {
            operation,
            path: path.to_path_buf(),
            source,
        }
    }
}

impl std::fmt::Display for PathIoError {
    /// Formats the operation, path, and native I/O error.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "failed to {} '{}': {}",
            self.operation,
            self.path.display(),
            self.source,
        )
    }
}

impl std::error::Error for PathIoError {
    /// Returns the native I/O error that caused this contextual error.
    #[inline(always)]
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

// This module verifies the private path-aware I/O wrapper and its source
// chaining. The public API cannot construct every intermediate wrapper state;
// making it public solely for tests would expand the error contract. Facade
// and native error integration tests cover the externally visible mapping.
#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;
    use std::io::ErrorKind;

    #[test]
    fn formats_context_and_exposes_source() {
        let source = Error::new(ErrorKind::PermissionDenied, "denied");
        let error = PathIoError::new("write", Path::new("file"), source);
        assert_eq!(error.to_string(), "failed to write 'file': denied");
        assert_eq!(
            error.source().expect("source is retained").to_string(),
            "denied"
        );
    }
}
