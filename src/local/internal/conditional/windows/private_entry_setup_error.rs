// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Error retaining both private-entry setup and identity-checked cleanup
//! failures.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Preserves a private-entry setup failure together with an unsafe-to-ignore
/// cleanup failure and the affected namespace path.
///
/// This internal error is used when setup fails and identity-checked cleanup
/// also fails, so callers can retain the original failure while reporting the
/// cleanup problem.
#[derive(Debug)]
pub(super) struct PrivateEntrySetupError {
    /// Failure that prevented private entry setup from completing.
    pub(super) primary: io::Error,
    /// Failure to prove and complete cleanup of the created entry.
    pub(super) cleanup: io::Error,
    /// Namespace location retained for diagnostics only.
    pub(super) path: PathBuf,
}

impl fmt::Display for PrivateEntrySetupError {
    /// Formats the primary failure and the failed cleanup location and cause.
    ///
    /// # Parameters
    ///
    /// * `formatter` is the destination that receives the combined diagnostic.
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

impl std::error::Error for PrivateEntrySetupError {
    /// Returns the setup failure that originally prevented entry creation.
    ///
    /// The cleanup failure and affected path remain available in the display
    /// message because this error retains both failures in one `io::Error`.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.primary)
    }
}
