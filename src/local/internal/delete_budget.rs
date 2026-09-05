// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared cooperative resource accounting for recursive deletion.
// qubit-style: allow source-test-pair
// Covered by public Host and Rooted deletion-budget integration tests.

use std::io;
use std::path::Path;
use std::time::Instant;

use qubit_budget::InsufficientBudgetError;
use qubit_budget::ResourceBudget;

use crate::LocalDeleteOptions;
use crate::LocalResourceKind;
use crate::LocalResourceLimitError;

/// Resource state for one recursive deletion, including pending work paths.
#[derive(Debug)]
pub(crate) struct DeleteBudget {
    /// Immutable caller-selected limits.
    options: LocalDeleteOptions,
    /// Monotonic operation start used by cooperative deadline checks.
    started: Instant,
    /// Discovered entries remaining in the operation budget.
    entries: Option<ResourceBudget<LocalResourceKind, usize>>,
    /// Encoded native bytes retained by pending work paths.
    pending_bytes: usize,
}

impl DeleteBudget {
    /// Creates a fresh budget; no filesystem access or mutation is performed.
    pub(crate) fn new(options: LocalDeleteOptions) -> Self {
        Self {
            options,
            started: Instant::now(),
            entries: options
                .max_entries()
                .map(|limit| ResourceBudget::new(LocalResourceKind::Entry, limit)),
            pending_bytes: 0,
        }
    }

    /// Stops work when its configured elapsed-time limit has been reached.
    ///
    /// Returns `TimedOut` before the next native call; in-flight I/O cannot
    /// be interrupted by this cooperative deadline.
    pub(crate) fn check_deadline(&self) -> io::Result<()> {
        // Exercise expiry between native calls without timing-dependent tests.
        #[cfg(feature = "test-support")]
        let forced_expiry = [
            ("local-delete-deadline-2", 2),
            ("local-delete-deadline-3", 3),
            ("local-delete-deadline-6", 6),
            ("local-delete-deadline-7", 7),
            ("local-delete-deadline-8", 8),
        ]
        .into_iter()
        .any(|(name, occurrence)| crate::local::take_test_support_on_nth(name, occurrence));
        #[cfg(not(feature = "test-support"))]
        let forced_expiry = false;
        if forced_expiry
            || self
                .options
                .deadline()
                .is_some_and(|limit| self.started.elapsed() >= limit)
        {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "recursive deletion deadline exceeded",
            ));
        }
        Ok(())
    }

    /// Charges a newly discovered entry before it is queued or inspected.
    ///
    /// `depth` is zero for the requested directory. Returns typed depth or
    /// entry budget facts, or `TimedOut`, without permitting unbudgeted work.
    pub(crate) fn discover(&mut self, depth: usize) -> io::Result<()> {
        self.check_deadline()?;
        if let Some(limit) = self.options.max_depth()
            && depth > limit
        {
            return Err(resource_error(LocalResourceLimitError::new(
                LocalResourceKind::Depth,
                limit,
                0,
                depth,
            )));
        }
        if let Some(entries) = self.entries.as_mut() {
            entries.try_consume(1).map_err(|error| {
                let InsufficientBudgetError {
                    resource,
                    limit,
                    remaining,
                    requested,
                } = error;
                resource_error(LocalResourceLimitError::new(resource, limit, remaining, requested))
            })?;
        }
        Ok(())
    }

    /// Reserves encoded path bytes before adding `path` to pending work.
    ///
    /// Returns structured memory-budget facts on exhaustion or accounting
    /// overflow. The actively inspected path is not part of this queue limit.
    pub(crate) fn reserve_path(&mut self, path: &Path) -> io::Result<()> {
        let requested = path.as_os_str().len();
        let limit = self.options.max_pending_path_bytes().unwrap_or(usize::MAX);
        let remaining = limit.saturating_sub(self.pending_bytes);
        if requested > remaining {
            return Err(resource_error(LocalResourceLimitError::new(
                LocalResourceKind::PendingPathBytes,
                limit,
                remaining,
                requested,
            )));
        }
        self.pending_bytes += requested;
        Ok(())
    }

    /// Releases a previously reserved path when its work item is popped.
    pub(crate) fn release_path(&mut self, path: &Path) {
        self.pending_bytes -= path.as_os_str().len();
    }
}

/// Wraps typed budget facts for conversion by the public filesystem facade.
fn resource_error(error: LocalResourceLimitError) -> io::Error {
    io::Error::new(io::ErrorKind::QuotaExceeded, error)
}
