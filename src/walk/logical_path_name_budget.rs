// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Combined expected-and-observed logical path name-byte budget.

use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::LocalResourceKind;
use crate::LocalResourceLimitError;
use crate::LocalResult;

/// Enforces tree validation's logical encoded-byte budget before Host-side
/// inspection/descent or Rooted path resolution.
#[derive(Debug)]
pub(super) struct LogicalPathNameBudget {
    /// Maximum combined encoded-byte count for expected and observed paths.
    limit: usize,
    /// Encoded bytes already charged to expected and observed paths.
    observed: usize,
    /// Namespace root used to report the exact path only when charging fails.
    diagnostic_root: PathBuf,
}

impl LogicalPathNameBudget {
    /// Creates a budget seeded with bytes already charged to expected paths.
    ///
    /// # Parameters
    ///
    /// - `observed`: Encoded bytes already charged to the expected paths.
    /// - `limit`: Maximum combined encoded-byte count for expected and observed
    ///   paths.
    /// - `diagnostic_root`: Namespace root for an over-budget path diagnostic.
    pub(super) fn new(observed: usize, limit: usize, diagnostic_root: PathBuf) -> Self {
        Self {
            limit,
            observed,
            diagnostic_root,
        }
    }

    /// Charges one relative path and reports the exact namespace path if it
    /// would exceed the combined expected-and-observed allowance.
    ///
    /// # Parameters
    ///
    /// - `relative`: Root-relative path whose encoded bytes are charged.
    pub(super) fn consume(&mut self, relative: &Path) -> LocalResult<()> {
        let requested = relative.as_os_str().as_encoded_bytes().len();
        let Some(observed) = self.observed.checked_add(requested) else {
            let path = self.diagnostic_root.join(relative);
            return Err(LocalFileError::from_resource_limit(
                LocalFileOperation::Read,
                Some(path),
                LocalResourceLimitError::new(LocalResourceKind::SeenNameBytes, usize::MAX, 0, 1).with_overflow(),
            )
            .with_reason("tree validation name-byte count overflowed"));
        };
        if observed > self.limit {
            let path = self.diagnostic_root.join(relative);
            return Err(LocalFileError::from_resource_limit(
                LocalFileOperation::Read,
                Some(path),
                LocalResourceLimitError::new(
                    LocalResourceKind::SeenNameBytes,
                    self.limit,
                    self.limit.saturating_sub(self.observed),
                    requested,
                )
                .with_observed(observed),
            )
            .with_reason("tree validation name-byte limit exceeded"));
        }
        self.observed = observed;
        Ok(())
    }
}
