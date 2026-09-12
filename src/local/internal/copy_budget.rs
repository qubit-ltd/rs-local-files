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

/// Maximum bytes requested from one blocking read.
const COPY_CHUNK_SIZE: usize = 64 * 1024;

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
        self.check_deadline_at(Instant::now())
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
        self.copy_with_now(reader, writer, Instant::now)
    }

    /// Copies in bounded chunks, checking the cooperative deadline at every
    /// read and write progress boundary.
    pub(crate) fn copy_with_now<R, W, N>(&mut self, reader: &mut R, writer: &mut W, mut now: N) -> io::Result<u64>
    where
        R: Read + ?Sized,
        W: Write + ?Sized,
        N: FnMut() -> Instant,
    {
        let mut buffer = [0_u8; COPY_CHUNK_SIZE];
        let mut copied = 0_u64;

        loop {
            self.check_deadline_at(now())?;
            let read_capacity = self.read_capacity(buffer.len());
            let read = match reader.read(&mut buffer[..read_capacity]) {
                Ok(0) => return Ok(copied),
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            self.check_deadline_at(now())?;

            let permitted = self.remaining_bytes().map_or(read, |remaining| {
                usize::try_from(remaining).unwrap_or(usize::MAX).min(read)
            });
            self.write_all_with_deadline(writer, &buffer[..permitted], &mut copied, &mut now)?;

            if permitted < read {
                return Err(self.copied_bytes_exhausted(read - permitted));
            }
        }
    }

    /// Checks the configured deadline against a supplied monotonic instant.
    #[inline]
    fn check_deadline_at(&self, now: Instant) -> io::Result<()> {
        if self
            .deadline
            .is_some_and(|(started, duration)| now.saturating_duration_since(started) >= duration)
        {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "local copy deadline exceeded"));
        }
        Ok(())
    }

    /// Returns the next read size, allowing one byte beyond a bounded source.
    #[inline]
    fn read_capacity(&self, chunk_size: usize) -> usize {
        self.remaining_bytes().map_or(chunk_size, |remaining| {
            usize::try_from(remaining.saturating_add(1))
                .unwrap_or(usize::MAX)
                .min(chunk_size)
        })
    }

    /// Returns the remaining copied-byte capacity when it is bounded.
    #[inline]
    fn remaining_bytes(&self) -> Option<u64> {
        self.bytes.as_ref().map(|budget| budget.remaining())
    }

    /// Writes a chunk completely while committing each successful partial
    /// write to the copied-byte budget.
    fn write_all_with_deadline<W, N>(
        &mut self,
        writer: &mut W,
        mut source: &[u8],
        copied: &mut u64,
        now: &mut N,
    ) -> io::Result<()>
    where
        W: Write + ?Sized,
        N: FnMut() -> Instant,
    {
        while !source.is_empty() {
            self.check_deadline_at(now())?;
            let written = match writer.write(source) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "failed to write copy staging data",
                    ));
                }
                Ok(written) => written,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            self.charge_bytes(written)?;
            *copied = copied.saturating_add(u64::try_from(written).unwrap_or(u64::MAX));
            source = &source[written..];
            self.check_deadline_at(now())?;
        }
        Ok(())
    }

    /// Commits actual staging bytes to the configured budget.
    #[inline]
    fn charge_bytes(&mut self, amount: usize) -> io::Result<()> {
        if let Some(budget) = self.bytes.as_mut() {
            budget
                .try_consume(u64::try_from(amount).unwrap_or(u64::MAX))
                .map_err(u64_budget_error)?;
        }
        Ok(())
    }

    /// Constructs the existing copied-byte exhaustion error after staging the
    /// portion that still fit.
    #[inline]
    fn copied_bytes_exhausted(&mut self, excess: usize) -> io::Error {
        self.bytes
            .as_mut()
            .expect("an over-limit read requires a copied-byte budget")
            .try_consume(u64::try_from(excess).unwrap_or(u64::MAX))
            .map_err(u64_budget_error)
            .expect_err("an over-limit read must exceed the exhausted copied-byte budget")
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
