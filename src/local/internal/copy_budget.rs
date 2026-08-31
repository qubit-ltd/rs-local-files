// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared resource accounting for Host and Rooted copy implementations.
// qubit-style: allow source-test-pair

use std::io;
use std::io::Read;
use std::io::Write;
use std::time::Duration;
use std::time::Instant;

use qubit_budget::InsufficientBudgetError;
use qubit_budget::ManagedResourcePermit;
use qubit_budget::ManagedResourcePool;
use qubit_budget::ResourceBudget;

use crate::LocalCopyDirOptions;
use crate::LocalResourceKind;
use crate::LocalResourceLimitError;

/// Mutable resource state shared by both native copy backends.
#[derive(Debug)]
pub struct CopyBudget {
    /// Maximum number of source entries that may be processed.
    entries: Option<ResourceBudget<LocalResourceKind, usize>>,

    /// Maximum number of source bytes that may be copied.
    bytes: Option<ResourceBudget<LocalResourceKind, u64>>,

    /// Concurrent source-directory reader capacity.
    open_directories: Option<ManagedResourcePool<LocalResourceKind, usize>>,

    /// Maximum source-entry depth beneath the copied root.
    max_depth: Option<usize>,

    /// Monotonic start and relative deadline for this copy.
    deadline: Option<(Instant, Duration)>,
}

impl CopyBudget {
    /// Creates budget state from validated internal copy options.
    ///
    /// The public copy facade rejects unrepresentable deadlines before
    /// dispatching to either backend.
    #[must_use]
    pub fn new(options: LocalCopyDirOptions) -> Self {
        Self {
            entries: options
                .max_entries()
                .map(|limit| ResourceBudget::new(LocalResourceKind::Entry, limit)),
            bytes: options
                .max_bytes()
                .map(|limit| ResourceBudget::new(LocalResourceKind::CopiedBytes, limit)),
            open_directories: options
                .max_open_directories()
                .map(|limit| ManagedResourcePool::new(LocalResourceKind::OpenDirectory, limit)),
            max_depth: options.max_depth(),
            deadline: options
                .deadline()
                .map(|duration| (options.started_at().unwrap_or_else(Instant::now), duration)),
        }
    }

    /// Rejects work after the configured monotonic deadline.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::TimedOut`] once the deadline is reached.
    #[inline]
    pub fn check_deadline(&self) -> io::Result<()> {
        if self
            .deadline
            .is_some_and(|(started, duration)| started.elapsed() >= duration)
        {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "local copy deadline exceeded"));
        }
        Ok(())
    }

    /// Checks whether a descendant entry is within the configured depth.
    ///
    /// # Parameters
    ///
    /// - `depth`: Descendant depth, where immediate children have depth one.
    ///
    /// # Errors
    ///
    /// Returns a structured resource-limit error when `depth` exceeds the
    /// configured maximum.
    #[inline]
    pub fn check_depth(&self, depth: usize) -> io::Result<()> {
        if let Some(limit) = self.max_depth
            && depth > limit
        {
            return Err(resource_error(LocalResourceLimitError::new(
                LocalResourceKind::Depth,
                limit,
                0,
                depth,
            )));
        }
        Ok(())
    }

    /// Charges one processed source entry.
    ///
    /// # Errors
    ///
    /// Returns a structured resource-limit error when no entry capacity
    /// remains.
    #[inline]
    pub fn charge_entry(&mut self) -> io::Result<()> {
        if let Some(budget) = self.entries.as_mut() {
            budget.try_consume(1).map_err(usize_budget_error)?;
        }
        Ok(())
    }

    /// Acquires capacity for one source-directory reader.
    ///
    /// # Returns
    ///
    /// `Some(permit)` when an open-directory limit is configured, or `None`
    /// when this dimension is unconfigured. Dropping the permit returns its
    /// capacity.
    ///
    /// # Errors
    ///
    /// Returns a structured resource-limit error when the directory capacity
    /// is exhausted.
    #[inline]
    pub fn acquire_directory(&self) -> io::Result<Option<ManagedResourcePermit<LocalResourceKind, usize>>> {
        self.open_directories
            .as_ref()
            .map(|pool| pool.try_acquire(1).map_err(usize_budget_error))
            .transpose()
    }

    /// Copies from an opened source while enforcing the actual byte limit.
    ///
    /// The bounded reader permits at most one byte beyond the remaining
    /// capacity so exhaustion is detected without publishing the staging file.
    ///
    /// # Parameters
    ///
    /// - `reader`: Open source positioned at the bytes to copy.
    /// - `writer`: Private destination staging writer.
    ///
    /// # Returns
    ///
    /// The exact number of bytes written to staging.
    ///
    /// # Errors
    ///
    /// Returns an I/O error from either descriptor, a deadline error, or a
    /// structured copied-byte resource-limit error.
    pub fn copy<R, W>(&mut self, reader: &mut R, writer: &mut W) -> io::Result<u64>
    where
        R: Read + ?Sized,
        W: Write + ?Sized,
    {
        self.check_deadline()?;
        let copied = match self.bytes.as_ref() {
            Some(budget) => {
                let read_limit = budget.remaining().saturating_add(1);
                io::copy(&mut reader.take(read_limit), writer)?
            }
            None => io::copy(reader, writer)?,
        };
        if let Some(budget) = self.bytes.as_mut() {
            budget.try_consume(copied).map_err(u64_budget_error)?;
        }
        self.check_deadline()?;
        Ok(copied)
    }
}

/// Wraps structured resource facts in the standard quota-exceeded channel.
#[inline]
fn resource_error(error: LocalResourceLimitError) -> io::Error {
    io::Error::new(io::ErrorKind::QuotaExceeded, error)
}

/// Converts a machine-sized budget failure without discarding its facts.
fn usize_budget_error(error: InsufficientBudgetError<LocalResourceKind, usize>) -> io::Error {
    let InsufficientBudgetError {
        resource,
        limit,
        remaining,
        requested,
    } = error;
    resource_error(LocalResourceLimitError::new(resource, limit, remaining, requested))
}

/// Converts a byte budget failure into structured machine-sized facts.
fn u64_budget_error(error: InsufficientBudgetError<LocalResourceKind, u64>) -> io::Error {
    let InsufficientBudgetError {
        resource,
        limit,
        remaining,
        requested,
    } = error;
    resource_error(LocalResourceLimitError::new(
        resource,
        usize::try_from(limit).unwrap_or(usize::MAX),
        usize::try_from(remaining).unwrap_or(usize::MAX),
        usize::try_from(requested).unwrap_or(usize::MAX),
    ))
}
