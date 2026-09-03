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
    pub(crate) fn into_parts(self) -> (RootedSymlinkCreateFailureState, io::Error, Option<io::Error>) {
        (self.state, self.primary, self.cleanup)
    }
}

impl Display for RootedSymlinkCreateError {
    /// Formats the proven publication state and retained native failures.
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
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.primary)
    }
}
