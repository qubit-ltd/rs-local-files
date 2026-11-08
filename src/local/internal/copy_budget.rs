// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared resource accounting for Host and Rooted copy implementations.

use std::io;
use std::time::Duration;
use std::time::Instant;

use qubit_budget::InsufficientBudgetError;
use qubit_budget::ManagedResourcePermit;
use qubit_budget::ManagedResourcePool;
use qubit_budget::ResourceBudget;
use qubit_io::ByteInput;
use qubit_io::ByteOutput;
use qubit_io::Input;
use qubit_io::Output;

use crate::LocalCopyDirOptions;
use crate::LocalDeleteOptions;
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
    ///
    /// # Parameters
    ///
    /// - `options`: Validated limits and deadline settings for one copy.
    ///
    /// # Returns
    ///
    /// A budget initialized with the configured source limits and deadline.
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
    /// # Returns
    ///
    /// `Ok(())` when the deadline has not been reached.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::TimedOut`] once the deadline is reached.
    #[inline]
    pub fn check_deadline(&self) -> io::Result<()> {
        let Some(deadline) = self.absolute_deadline()? else {
            return Ok(());
        };
        #[cfg(test)]
        let now = crate::tests::rooted::support::resolution_observation_tests::copy_budget_now(Some(deadline));
        #[cfg(not(test))]
        let now = Instant::now();
        if now >= deadline {
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
    ///
    /// # Returns
    ///
    /// `Ok(())` when the depth is allowed.
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
    /// # Returns
    ///
    /// `Ok(())` when an entry is charged successfully.
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
    /// # Type Parameters
    ///
    /// - `R`: Byte input providing the source data.
    /// - `W`: Byte output receiving the private staging data.
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
        R: ByteInput + ?Sized,
        W: ByteOutput + ?Sized,
    {
        self.copy_with_now(reader, writer, Instant::now)
    }

    /// Projects the copy deadline onto recursive target deletion without
    /// applying source-entry, depth, or open-directory limits to the target.
    ///
    /// The returned instant is the original copy start when a deadline is
    /// configured; otherwise deletion remains unbounded.
    ///
    /// # Returns
    ///
    /// Recursive deletion options and the copy's original start instant, or
    /// the current instant when no copy deadline is configured.
    #[must_use = "the copy budget's deletion limits and original start time should be used"]
    pub(crate) fn deletion_options(&self) -> (LocalDeleteOptions, Instant) {
        let options = LocalDeleteOptions::new().with_recursive();
        match self.deadline {
            Some((started, duration)) => (options.with_deadline(duration), started),
            None => (options, Instant::now()),
        }
    }

    /// Returns the original copy deadline as an absolute monotonic instant.
    ///
    /// # Returns
    ///
    /// The absolute deadline, or `None` when no deadline is configured.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::InvalidInput`] when the configured deadline is
    /// outside the representable monotonic clock range.
    pub(crate) fn absolute_deadline(&self) -> io::Result<Option<Instant>> {
        match self.deadline {
            None => Ok(None),
            Some((started, duration)) => started.checked_add(duration).map(Some).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "copy deadline exceeds the monotonic clock range",
                )
            }),
        }
    }

    /// Copies in bounded chunks, checking the cooperative deadline at every
    /// read and write progress boundary.
    ///
    /// # Type Parameters
    ///
    /// - `R`: Byte input providing the source data.
    /// - `W`: Byte output receiving the private staging data.
    /// - `N`: Clock callback returning the current monotonic instant.
    ///
    /// # Parameters
    ///
    /// - `reader`: Source positioned at the bytes to copy.
    /// - `writer`: Private destination staging writer.
    /// - `now`: Clock callback used for cooperative deadline checks.
    ///
    /// # Returns
    ///
    /// The number of bytes written to staging.
    ///
    /// # Errors
    ///
    /// Returns an I/O error, a deadline error, or a structured copied-byte
    /// resource-limit error.
    pub(crate) fn copy_with_now<R, W, N>(&mut self, reader: &mut R, writer: &mut W, mut now: N) -> io::Result<u64>
    where
        R: ByteInput + ?Sized,
        W: ByteOutput + ?Sized,
        N: FnMut() -> Instant,
    {
        let mut buffer = [0_u8; COPY_CHUNK_SIZE];
        let mut copied = 0_u64;

        loop {
            self.check_deadline_at(now())?;
            let read_capacity = self.read_capacity(buffer.len());
            let read = match Input::read(reader, &mut buffer[..read_capacity]) {
                Ok(0) => {
                    self.check_deadline_at(now())?;
                    return Ok(copied);
                }
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
    ///
    /// # Parameters
    ///
    /// - `now`: Monotonic instant to compare with the configured deadline.
    ///
    /// # Returns
    ///
    /// `Ok(())` when the configured deadline has not been reached.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::TimedOut`] when the supplied instant reaches
    /// the configured deadline.
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
    ///
    /// # Parameters
    ///
    /// - `chunk_size`: Maximum size of the current read buffer.
    ///
    /// # Returns
    ///
    /// A read size no larger than `chunk_size`.
    #[inline]
    fn read_capacity(&self, chunk_size: usize) -> usize {
        self.remaining_bytes().map_or(chunk_size, |remaining| {
            usize::try_from(remaining.saturating_add(1))
                .unwrap_or(usize::MAX)
                .min(chunk_size)
        })
    }

    /// Returns the remaining copied-byte capacity when it is bounded.
    ///
    /// # Returns
    ///
    /// `Some(remaining)` when byte copying is bounded, otherwise `None`.
    #[inline]
    fn remaining_bytes(&self) -> Option<u64> {
        self.bytes.as_ref().map(|budget| budget.remaining())
    }

    /// Writes a chunk completely while committing each successful partial
    /// write to the copied-byte budget.
    ///
    /// # Type Parameters
    ///
    /// - `W`: Byte output receiving the source slice.
    /// - `N`: Clock callback returning the current monotonic instant.
    ///
    /// # Parameters
    ///
    /// - `writer`: Destination staging writer.
    /// - `source`: Remaining bytes in the chunk to write.
    /// - `copied`: Total bytes committed to staging so far.
    /// - `now`: Clock callback used before and after write progress.
    ///
    /// # Returns
    ///
    /// `Ok(())` after the complete source slice has been written.
    ///
    /// # Errors
    ///
    /// Returns an I/O error, a deadline error, or a copied-byte budget error.
    fn write_all_with_deadline<W, N>(
        &mut self,
        writer: &mut W,
        mut source: &[u8],
        copied: &mut u64,
        now: &mut N,
    ) -> io::Result<()>
    where
        W: ByteOutput + ?Sized,
        N: FnMut() -> Instant,
    {
        while !source.is_empty() {
            self.check_deadline_at(now())?;
            let written = match Output::write(writer, source) {
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
    ///
    /// # Parameters
    ///
    /// - `amount`: Number of bytes successfully written to staging.
    ///
    /// # Returns
    ///
    /// `Ok(())` when the amount is charged successfully.
    ///
    /// # Errors
    ///
    /// Returns a structured copied-byte resource-limit error if the amount
    /// exceeds the remaining capacity.
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
    ///
    /// # Parameters
    ///
    /// - `excess`: Number of observed source bytes beyond the remaining limit.
    ///
    /// # Returns
    ///
    /// A quota-exceeded error retaining the copied-byte budget facts.
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
///
/// # Parameters
///
/// - `error`: Structured facts describing the exceeded resource limit.
///
/// # Returns
///
/// An I/O error in the quota-exceeded channel containing the supplied facts.
#[inline]
fn resource_error(error: LocalResourceLimitError) -> io::Error {
    io::Error::new(io::ErrorKind::QuotaExceeded, error)
}

/// Converts a machine-sized budget failure without discarding its facts.
///
/// # Parameters
///
/// - `error`: Machine-sized budget failure with resource and limit facts.
///
/// # Returns
///
/// An I/O error in the quota-exceeded channel retaining the resource facts.
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
///
/// # Parameters
///
/// - `error`: Byte-sized budget failure with resource and limit facts.
///
/// # Returns
///
/// An I/O error in the quota-exceeded channel retaining converted resource
/// facts.
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::time::Duration;
    use std::time::Instant;

    use qubit_io::Output;

    use super::CopyBudget;
    use super::io;
    use crate::local::LocalCopyDirOptions;

    struct TwoThenBlock {
        bytes: Vec<u8>,
    }

    impl Output for TwoThenBlock {
        type Item = u8;

        unsafe fn write_unchecked(&mut self, input: &[u8], index: usize, count: usize) -> io::Result<usize> {
            if !self.bytes.is_empty() {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            let accepted = count.min(2);
            self.bytes.extend_from_slice(&input[index..index + accepted]);
            Ok(accepted)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct DeadlineAfterPartialWrite {
        bytes: Vec<u8>,
    }

    impl Output for DeadlineAfterPartialWrite {
        type Item = u8;

        unsafe fn write_unchecked(&mut self, input: &[u8], index: usize, count: usize) -> io::Result<usize> {
            let accepted = count.min(2);
            self.bytes.extend_from_slice(&input[index..index + accepted]);
            Ok(accepted)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_copy_budget_charges_partial_write_before_would_block() {
        let mut budget = CopyBudget::new(LocalCopyDirOptions::new().with_max_bytes(3));
        let mut reader = Cursor::new(b"abcde");
        let mut writer = TwoThenBlock { bytes: Vec::new() };

        let error = budget.copy(&mut reader, &mut writer).unwrap_err();

        assert_eq!(io::ErrorKind::WouldBlock, error.kind());
        assert_eq!(b"ab", writer.bytes.as_slice());
        assert_eq!(Some(1), budget.remaining_bytes());
    }

    #[test]
    fn test_copy_budget_checks_deadline_after_partial_write() {
        let started = Instant::now();
        let mut budget = CopyBudget::new(
            LocalCopyDirOptions::new()
                .with_max_bytes(3)
                .with_deadline(Duration::from_millis(1))
                .with_started_at(started),
        );
        let mut reader = Cursor::new(b"abcd");
        let mut writer = DeadlineAfterPartialWrite { bytes: Vec::new() };
        let mut calls = 0;
        let mut now = || {
            calls += 1;
            if calls < 4 {
                started
            } else {
                started + Duration::from_millis(1)
            }
        };

        let error = budget.copy_with_now(&mut reader, &mut writer, &mut now).unwrap_err();

        assert_eq!(io::ErrorKind::TimedOut, error.kind());
        assert_eq!(b"ab", writer.bytes.as_slice());
        assert_eq!(Some(1), budget.remaining_bytes());
    }

    #[test]
    fn test_copy_budget_accepts_exact_limit_and_rejects_one_byte_over() {
        let mut exact_budget = CopyBudget::new(LocalCopyDirOptions::new().with_max_bytes(3));
        let mut exact_reader = Cursor::new(b"abc");
        let mut exact_writer = Vec::new();
        assert_eq!(3, exact_budget.copy(&mut exact_reader, &mut exact_writer).unwrap());
        assert_eq!(Some(0), exact_budget.remaining_bytes());

        let mut over_budget = CopyBudget::new(LocalCopyDirOptions::new().with_max_bytes(3));
        let mut over_reader = Cursor::new(b"abcd");
        let mut over_writer = Vec::new();
        let error = over_budget.copy(&mut over_reader, &mut over_writer).unwrap_err();
        assert_eq!(io::ErrorKind::QuotaExceeded, error.kind());
        assert_eq!(b"abc", over_writer.as_slice());
        assert_eq!(Some(0), over_budget.remaining_bytes());
        let facts = error
            .get_ref()
            .and_then(|source| source.downcast_ref::<crate::LocalResourceLimitError>())
            .expect("over-budget error should retain structured facts");
        assert_eq!(crate::LocalResourceKind::CopiedBytes, facts.resource());
        assert_eq!(3, facts.limit());
        assert_eq!(0, facts.remaining());
        assert_eq!(1, facts.requested());
    }
}
