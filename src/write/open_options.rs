// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native write-open options.

use std::time::Duration;

use super::Mode;

/// Configures one native local file write-open operation.
#[must_use = "write-open options have no effect unless they are used"]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenOptions {
    /// Native creation and positioning behavior.
    mode: Mode,
    /// Whether missing parent directories are created before opening.
    create_parents: bool,
    /// Maximum time spent retrying Unix lease-conflicting opens.
    open_retry_timeout: Duration,
}

impl OpenOptions {
    /// Creates options for one native write mode.
    ///
    /// # Parameters
    /// - `mode`: Native creation and positioning behavior.
    ///
    /// # Returns
    /// Options without parent creation or implicit retry.
    #[inline]
    pub const fn new(mode: Mode) -> Self {
        Self {
            mode,
            create_parents: false,
            open_retry_timeout: Duration::ZERO,
        }
    }

    /// Returns the native write mode.
    #[inline]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Returns whether missing parents are created.
    #[must_use]
    #[inline]
    pub const fn creates_parents(&self) -> bool {
        self.create_parents
    }

    /// Enables creation of missing parent directories.
    ///
    /// # Returns
    /// Updated options.
    #[inline]
    pub const fn with_parents(mut self) -> Self {
        self.create_parents = true;
        self
    }

    /// Returns the Unix lease-conflict retry timeout.
    #[must_use]
    #[inline]
    pub const fn open_retry_timeout(&self) -> Duration {
        self.open_retry_timeout
    }

    /// Sets the Unix lease-conflict retry timeout.
    ///
    /// # Parameters
    /// - `timeout`: Maximum retry duration; zero disables retry.
    ///
    /// # Returns
    /// Updated options.
    #[inline]
    pub const fn with_open_retry_timeout(mut self, timeout: Duration) -> Self {
        self.open_retry_timeout = timeout;
        self
    }
}

impl Default for OpenOptions {
    /// Creates or truncates a file without parent creation or retry.
    #[inline]
    fn default() -> Self {
        Self::new(Mode::default())
    }
}
