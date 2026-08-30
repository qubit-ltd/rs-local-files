// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by host and rooted reader integration tests.

use std::time::Duration;

/// Options for opening a native local file reader.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "read options have no effect unless they are used"]
pub struct LocalReadOptions {
    /// Optional maximum time spent retrying Unix lease conflicts.
    open_retry_timeout: Option<Duration>,
}

impl LocalReadOptions {
    /// Creates default reader options that perform only the initial open.
    pub const fn new() -> Self {
        Self {
            open_retry_timeout: None,
        }
    }

    /// Returns the configured Unix open retry timeout.
    ///
    /// `None` means the library does not retry.
    #[must_use]
    pub const fn open_retry_timeout(&self) -> Option<Duration> {
        self.open_retry_timeout
    }

    /// Sets the maximum time spent retrying Unix lease conflicts.
    ///
    /// # Parameters
    ///
    /// - `timeout`: Retry duration; zero permits the initial attempt only.
    ///
    /// # Returns
    ///
    /// Updated reader options.
    pub const fn with_open_retry_timeout(mut self, timeout: Duration) -> Self {
        self.open_retry_timeout = Some(timeout);
        self
    }

    /// Disables library-level open retries.
    pub const fn without_open_retry_timeout(mut self) -> Self {
        self.open_retry_timeout = None;
        self
    }
}
