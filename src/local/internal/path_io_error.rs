// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Path-aware I/O error context.

use std::io::Error;
use std::path::{
    Path,
    PathBuf,
};

/// An I/O error annotated with the failed operation and path.
#[derive(Debug)]
pub(super) struct PathIoError {
    operation: &'static str,
    path: PathBuf,
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
    pub(super) fn new(
        operation: &'static str,
        path: &Path,
        source: Error,
    ) -> Self {
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
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}
