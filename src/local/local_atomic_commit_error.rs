// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recoverable atomic-commit errors.
// qubit-style: allow source-test-pair
// qubit-style: allow inline-tests
// qubit-style: allow explicit-imports

use std::error::Error;
use std::fmt::{
    Debug,
    Display,
    Formatter,
    Result as FmtResult,
};

use crate::LocalAtomicWriteError;

/// Atomic-commit failure that optionally returns its staging writer.
///
/// Failures detected before the installation attempt retain the writer for
/// retry or explicit abort. Failures after installation begins are terminal,
/// so [`Self::writer`] returns `None`.
///
/// # Type Parameters
///
/// * `T` - Type of staging writer retained for recovery.
#[non_exhaustive]
#[derive(Debug)]
pub(crate) struct LocalAtomicCommitError<T> {
    /// Structured atomic-write failure.
    error: LocalAtomicWriteError,
    /// Writer retained after a recoverable pre-installation failure.
    writer: Option<Box<T>>,
}

#[allow(dead_code)]
impl<T> LocalAtomicCommitError<T> {
    /// Creates an atomic-commit error with an optional retained writer.
    ///
    /// # Parameters
    ///
    /// * `error` - Structured atomic-write failure.
    /// * `writer` - Writer retained when retry or explicit abort remains safe.
    ///
    /// # Returns
    ///
    /// A commit error preserving the failure and optional writer.
    #[inline]
    pub(crate) fn new(error: LocalAtomicWriteError, writer: Option<T>) -> Self {
        Self {
            error,
            writer: writer.map(Box::new),
        }
    }

    /// Returns the structured atomic-write failure.
    ///
    /// # Returns
    ///
    /// The failure produced by the commit attempt.
    #[must_use]
    pub(crate) const fn error(&self) -> &LocalAtomicWriteError {
        &self.error
    }

    /// Returns the retained writer when recovery remains safe.
    ///
    /// # Returns
    ///
    /// `Some` for a pre-installation failure that permits retry or explicit
    /// abort, or `None` after installation began.
    #[must_use]
    pub(crate) fn writer(&self) -> Option<&T> {
        self.writer.as_deref()
    }

    /// Returns the retained writer mutably when recovery remains safe.
    ///
    /// # Returns
    ///
    /// `Some` for a pre-installation failure that permits additional staging
    /// writes, retry, or explicit abort, or `None` after installation began.
    #[must_use]
    pub(crate) fn writer_mut(&mut self) -> Option<&mut T> {
        self.writer.as_deref_mut()
    }

    /// Splits this error into its structured failure and retained writer.
    ///
    /// # Returns
    ///
    /// The atomic-write failure and the writer retained for recovery, when
    /// recovery remains safe.
    #[must_use = "the returned writer may require retry or explicit abort"]
    pub(crate) fn into_parts(self) -> (LocalAtomicWriteError, Option<T>) {
        let Self { error, writer } = self;
        (error, writer.map(|writer| *writer))
    }

    /// Converts this recoverable commit error into a consuming commit failure.
    ///
    /// # Parameters
    ///
    /// * `finalize_writer` - Finalizes a retained writer and enriches its
    ///   structured failure with any cleanup error.
    ///
    /// # Returns
    ///
    /// The finalized writer failure when recovery remained available, or the
    /// original terminal failure when no writer was retained.
    #[inline]
    pub(crate) fn into_final_error_with<F>(
        self,
        finalize_writer: F,
    ) -> LocalAtomicWriteError
    where
        F: FnOnce(T, LocalAtomicWriteError) -> LocalAtomicWriteError,
    {
        let (error, writer) = self.into_parts();
        match writer {
            Some(writer) => finalize_writer(writer, error),
            None => error,
        }
    }
}

impl<T> Display for LocalAtomicCommitError<T> {
    /// Formats the structured atomic-write failure and recovery availability.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        if self.writer.is_some() {
            write!(formatter, "{}; staging writer retained", self.error)
        } else {
            write!(formatter, "{}; staging writer unavailable", self.error)
        }
    }
}

impl<T> Error for LocalAtomicCommitError<T>
where
    T: Debug,
{
    /// Returns the structured atomic-write failure.
    #[inline(always)]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

// These tests exercise private commit-state transitions and ownership splits.
// Public callers observe only terminal outcomes, so the intermediate states
// cannot be verified through public APIs. Adding test-only constructors would
// weaken the state machine; writer integration tests cover terminal behavior.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        LocalAtomicDestinationState,
        LocalAtomicWriteStage,
    };
    use std::io;

    fn error() -> LocalAtomicWriteError {
        LocalAtomicWriteError::new(
            LocalAtomicWriteStage::ReplaceDestination,
            "target".into(),
            None,
            LocalAtomicDestinationState::Unchanged,
            io::Error::other("boom"),
        )
    }

    #[test]
    fn retains_and_splits_recoverable_writer() {
        let mut commit = LocalAtomicCommitError::new(error(), Some(7_u8));
        assert_eq!(commit.writer(), Some(&7));
        assert_eq!(commit.writer_mut().map(|value| *value), Some(7));
        assert!(commit.to_string().contains("retained"));
        assert!(std::error::Error::source(&commit).is_some());
        let (error, writer) = commit.into_parts();
        assert_eq!(writer, Some(7));
        assert_eq!(error.kind(), io::ErrorKind::Other);
    }

    #[test]
    fn finalizes_or_returns_terminal_error() {
        let result = LocalAtomicCommitError::new(error(), Some(3_u8))
            .into_final_error_with(|writer, error| {
                assert_eq!(writer, 3);
                error
            });
        assert_eq!(result.kind(), io::ErrorKind::Other);
        let terminal = LocalAtomicCommitError::<u8>::new(error(), None);
        assert!(terminal.writer().is_none());
        assert!(terminal.to_string().contains("unavailable"));
        let _ = terminal.into_final_error_with(|_, error| error);
    }
}
