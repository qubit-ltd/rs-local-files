// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Structured failure facts for rooted symbolic-link publication.

use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::io;

use super::RootedSymlinkCreateFailureState;

/// Failure from rooted symbolic-link publication and optional rollback.
#[must_use = "the publication and cleanup failures must be handled"]
#[derive(Debug)]
pub(crate) struct RootedSymlinkCreateError {
    /// Strongest destination state proven after publication and rollback.
    state: RootedSymlinkCreateFailureState,
    /// Native error that prevented symbolic-link publication.
    primary: io::Error,
    /// Native rollback error retained without replacing the primary failure.
    cleanup: Option<io::Error>,
}

impl RootedSymlinkCreateError {
    /// Creates a failure with the strongest proven namespace state.
    ///
    /// # Parameters
    ///
    /// - `state` is the strongest destination state proven after publication
    ///   and rollback.
    /// - `primary` is the native error that prevented symbolic-link
    ///   publication.
    /// - `cleanup` is an optional native error from placeholder rollback.
    ///
    /// # Returns
    ///
    /// The error retaining the publication state and both native failures.
    pub(crate) const fn new(
        state: RootedSymlinkCreateFailureState,
        primary: io::Error,
        cleanup: Option<io::Error>,
    ) -> Self {
        Self {
            state,
            primary,
            cleanup,
        }
    }

    /// Decomposes this failure into publication state and native errors.
    ///
    /// # Parameters
    ///
    /// - `self` transfers ownership of the retained state and errors.
    ///
    /// # Returns
    ///
    /// The publication state, primary failure, and optional cleanup failure.
    #[must_use = "the retained publication and cleanup errors must be handled"]
    pub(crate) fn into_parts(self) -> (RootedSymlinkCreateFailureState, io::Error, Option<io::Error>) {
        (self.state, self.primary, self.cleanup)
    }
}

impl Display for RootedSymlinkCreateError {
    /// Formats the proven publication state and retained native failures.
    ///
    /// # Parameters
    ///
    /// - `formatter` receives the formatted state and failure details.
    ///
    /// # Returns
    ///
    /// The result of writing the error description.
    ///
    /// # Errors
    ///
    /// Returns the formatting error if writing to `formatter` fails.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(
            formatter,
            "rooted symbolic-link publication failed with {:?} state: {}",
            self.state, self.primary,
        )?;
        if let Some(cleanup) = self.cleanup.as_ref() {
            write!(formatter, "; placeholder cleanup also failed: {cleanup}")?;
        }
        Ok(())
    }
}

impl Error for RootedSymlinkCreateError {
    /// Returns the primary native publication failure.
    ///
    /// # Returns
    ///
    /// `Some` contains the primary publication error; this implementation
    /// never returns `None`.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.primary)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io;

    use super::RootedSymlinkCreateError;
    use crate::local::internal::RootedSymlinkCreateFailureState;

    #[test]
    fn test_display_and_source_retain_publication_and_cleanup_errors() {
        let error = RootedSymlinkCreateError::new(
            RootedSymlinkCreateFailureState::Unchanged,
            io::Error::other("publish failed"),
            Some(io::Error::other("cleanup failed")),
        );

        let message = error.to_string();
        assert!(message.contains("Unchanged"));
        assert!(message.contains("publish failed"));
        assert!(message.contains("cleanup failed"));
        assert_eq!(
            "publish failed",
            error.source().expect("primary error source").to_string()
        );
    }
}
