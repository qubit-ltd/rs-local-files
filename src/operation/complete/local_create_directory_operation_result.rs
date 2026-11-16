// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::outcome::LocalCreateDirectoryOutcome;

/// Successful directory creation paired with its caller-visible path.
#[derive(Debug)]
pub struct LocalCreateDirectoryOperationResult {
    /// Stores the path associated with this value.
    pub path: PathBuf,
    /// Stores the outcome associated with this value.
    pub outcome: LocalCreateDirectoryOutcome,
}
