// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native write-open options.
// qubit-style: allow source-test-pair
// qubit-style: allow explicit-imports

use std::time::Duration;

use super::Mode;

/// Configures one native local file write-open operation.
#[must_use = "write-open options have no effect unless they are used"]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OpenOptions {
    /// Native creation and positioning behavior.
    mode: Mode,
    /// Whether missing parent directories are created before opening.
    create_parents: bool,
    /// Optional maximum time spent retrying Unix lease-conflicting opens.
    open_retry_timeout: Option<Duration>,
}

impl OpenOptions {
    /// Creates options for one native write mode.
    ///
    /// # Parameters
    /// - `mode`: Native creation and positioning behavior.
    ///
    /// # Returns
    /// Options without parent creation and with ordinary unbounded open retry.
    pub(crate) const fn new(mode: Mode) -> Self {
        Self {
            mode,
            create_parents: false,
            open_retry_timeout: None,
        }
    }

    /// Returns the native write mode.
    pub(crate) const fn mode(&self) -> Mode {
        self.mode
    }

    /// Returns whether missing parents are created.
    #[must_use]
    pub(crate) const fn creates_parents(&self) -> bool {
        self.create_parents
    }

    /// Enables creation of missing parent directories.
    ///
    /// # Returns
    /// Updated options.
    #[allow(dead_code)]
    pub(crate) const fn with_parents(mut self) -> Self {
        self.create_parents = true;
        self
    }

    /// Returns the Unix lease-conflict retry timeout.
    ///
    /// `None` preserves ordinary unbounded blocking-open behavior. `Some`
    /// bounds retries, and a zero duration reports the first conflict.
    #[must_use]
    pub(crate) const fn open_retry_timeout(&self) -> Option<Duration> {
        self.open_retry_timeout
    }

    /// Sets the Unix lease-conflict retry timeout.
    ///
    /// # Parameters
    /// - `timeout`: Maximum retry duration; zero disables retry.
    ///
    /// # Returns
    /// Updated options.
    pub(crate) const fn with_open_retry_timeout(
        mut self,
        timeout: Duration,
    ) -> Self {
        self.open_retry_timeout = Some(timeout);
        self
    }
}

impl Default for OpenOptions {
    /// Creates or truncates a file without parent creation and with ordinary
    /// unbounded open retry.
    fn default() -> Self {
        Self::new(Mode::default())
    }
}
