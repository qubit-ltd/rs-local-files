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
    /// Optional maximum time spent retrying Unix lease-conflicting opens.
    open_retry_timeout: Option<Duration>,
}

impl OpenOptions {
    /// Returns the Unix lease-conflict retry timeout.
    ///
    /// `None` preserves ordinary unbounded blocking-open behavior. `Some`
    /// bounds retries, and a zero duration reports the first conflict.
    #[must_use]
    #[inline(always)]
    pub const fn open_retry_timeout(&self) -> Option<Duration> {
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
        self.open_retry_timeout = Some(timeout);
        self
    }
}
