// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Structured evidence that a complete read exceeded its configured byte limit.

use std::error::Error;
use std::fmt;

/// Observed read length that exceeded a configured byte limit.
///
/// `observed` is the length observed during metadata preflight or a proven
/// lower bound from reading; it is not necessarily the file's final length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalReadLimitError {
    /// Configured maximum byte count.
    limit: u64,
    /// Observed length or proven lower bound.
    observed: u64,
}

impl LocalReadLimitError {
    /// Creates a read-limit diagnostic.
    ///
    /// # Parameters
    /// - `limit`: Configured maximum byte count.
    /// - `observed`: Observed length or proven lower bound.
    ///
    /// # Returns
    /// The supplied read-limit facts.
    #[must_use]
    pub const fn new(limit: u64, observed: u64) -> Self {
        Self { limit, observed }
    }

    /// Returns the configured maximum byte count.
    ///
    /// # Returns
    /// The limit used for the read.
    #[must_use]
    pub const fn limit(&self) -> u64 {
        self.limit
    }

    /// Returns the observed length or proven lower bound.
    ///
    /// # Returns
    /// A metadata length or a lower bound proven by reading.
    #[must_use]
    pub const fn observed(&self) -> u64 {
        self.observed
    }
}

impl fmt::Display for LocalReadLimitError {
    /// Formats the observed and configured lengths.
    ///
    /// # Parameters
    /// - `formatter`: Destination formatter.
    ///
    /// # Returns
    /// The formatter's result.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "observed read length {} exceeds limit {}",
            self.observed, self.limit
        )
    }
}

impl Error for LocalReadLimitError {}
