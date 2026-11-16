// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::outcome::LocalRenameOutcome;

/// Successful rename paired with its caller-visible source and destination.
#[derive(Debug)]
pub struct LocalRenameOperationResult {
    /// Stores the source associated with this value.
    pub source: PathBuf,
    /// Stores the destination associated with this value.
    pub destination: PathBuf,
    /// Stores the outcome associated with this value.
    pub outcome: LocalRenameOutcome,
}
