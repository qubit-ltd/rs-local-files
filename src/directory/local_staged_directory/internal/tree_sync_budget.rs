// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;
use std::time::Instant;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::error::LocalResult;
use crate::options::LocalListOptions;

/// Tracks the cooperative deadline shared by staged-tree synchronization
/// passes.
#[derive(Debug)]
pub(in crate::directory) struct TreeSyncBudget {
    /// Absolute deadline derived from the operation's start time and list
    /// options.
    deadline: Option<Instant>,
}

impl TreeSyncBudget {
    /// Creates a deadline budget from one operation start instant.
    ///
    /// # Parameters
    /// `started_at` is the instant at which the synchronization operation
    /// began. `limits` supplies the optional duration used to derive the
    /// deadline.
    ///
    /// # Returns
    /// Returns a budget with no deadline when `limits` has no deadline
    /// duration.
    ///
    /// # Errors
    /// Returns `InvalidOptions` when adding the configured duration overflows.
    pub(in crate::directory) fn new(started_at: Instant, limits: &LocalListOptions) -> LocalResult<Self> {
        let deadline = limits
            .deadline()
            .map(|duration| {
                started_at
                    .checked_add(duration)
                    .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::Commit))
            })
            .transpose()?;
        Ok(Self { deadline })
    }

    /// Rejects work once the shared deadline has elapsed.
    ///
    /// # Parameters
    /// `path` identifies the operation location attached to a timeout error.
    ///
    /// # Returns
    /// Returns `Ok(())` while work remains within the configured deadline.
    ///
    /// # Errors
    /// Returns a commit timeout error when the deadline has elapsed.
    pub(in crate::directory) fn check(&self, path: &Path) -> LocalResult<()> {
        #[cfg(test)]
        let test_expired = super::super::sync::TREE_SYNC_TEST_EXPIRED.with(std::cell::Cell::get);
        #[cfg(not(test))]
        let test_expired = false;
        if test_expired || self.deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(LocalFileError::from_io(
                LocalFileOperation::Commit,
                Some(path.to_path_buf()),
                None,
                std::io::Error::new(std::io::ErrorKind::TimedOut, "tree synchronization deadline exceeded"),
            ));
        }
        Ok(())
    }
}
