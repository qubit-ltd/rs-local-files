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
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.primary)
    }
}
// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.

//    SPDX-License-Identifier: Apache-2.0

//    Licensed under the Apache License, Version 2.0.
// =============================================================================
