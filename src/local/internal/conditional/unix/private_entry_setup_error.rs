// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Diagnostic error retaining setup and verified-cleanup failures.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;

/// Preserves both the setup failure and a failure to clean up its private
/// entry.
#[derive(Debug)]
pub(super) struct PrivateEntrySetupError {
    /// Error that prevented private-entry setup from completing.
    pub(super) primary: io::Error,
    /// Error from identity verification or removal during cleanup.
    pub(super) cleanup: io::Error,
    /// Entry location included for diagnostics, not used for filesystem access.
    pub(super) path: PathBuf,
}

impl fmt::Display for PrivateEntrySetupError {
    /// Formats both failures and the affected diagnostic path.
    ///
    /// # Errors
    ///
    /// Returns the formatter error if writing the diagnostic fails.
    ///
    /// # Parameters
    ///
    /// `formatter` receives the combined setup and cleanup diagnostic.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}; private-entry cleanup failed at {}: {}",
            self.primary,
            self.path.display(),
            self.cleanup
        )
    }
}

impl Error for PrivateEntrySetupError {
    /// Returns the setup failure as the source; cleanup remains in the message.
    ///
    /// # Returns
    ///
    /// Returns `Some` with the primary setup failure. The cleanup failure is
    /// retained in the displayed diagnostic rather than as the source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.primary)
    }
}
