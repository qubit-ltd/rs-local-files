// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native read-open options.

use std::time::Duration;

/// Configures one native local file read-open operation.
#[must_use = "read-open options have no effect unless they are used"]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OpenOptions {
    /// Maximum time spent retrying Unix lease-conflicting opens.
    open_retry_timeout: Duration,
}

impl OpenOptions {
    /// Returns the Unix lease-conflict retry timeout.
    ///
    /// A zero duration disables retry after the first conflicting attempt.
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
