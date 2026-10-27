// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered by host and rooted reader integration tests.

use std::time::Duration;

use crate::policy::LocalSymlinkPolicy;

/// Options for opening a native local file reader.
///
/// # Examples
///
/// ```
/// use qubit_local_files::options::LocalReadOptions;
/// use std::time::Duration;
///
/// let options = LocalReadOptions::new().with_open_retry_timeout(Duration::from_secs(1));
/// assert_eq!(options.open_retry_timeout(), Some(Duration::from_secs(1)));
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "read options have no effect unless they are used"]
pub struct LocalReadOptions {
    /// Optional maximum time spent retrying Unix lease conflicts.
    open_retry_timeout: Option<Duration>,
    /// Optional symbolic-link policy override.
    symlink_policy: Option<LocalSymlinkPolicy>,
}

impl LocalReadOptions {
    /// Creates default reader options that perform only the initial open.
    #[inline]
    pub const fn new() -> Self {
        Self {
            open_retry_timeout: None,
            symlink_policy: None,
        }
    }

    /// Returns the configured Unix open retry timeout.
    ///
    /// `None` means the library does not retry.
    #[inline]
    pub const fn open_retry_timeout(&self) -> Option<Duration> {
        self.open_retry_timeout
    }

    /// Returns the symbolic-link policy override, if configured.
    #[inline]
    pub const fn symlink_policy(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink_policy
    }

    /// Sets the symbolic-link policy override for readers opened with these
    /// options.
    #[inline]
    pub const fn with_symlink_policy(mut self, policy: LocalSymlinkPolicy) -> Self {
        self.symlink_policy = Some(policy);
        self
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
