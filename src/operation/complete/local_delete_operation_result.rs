// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::outcome::LocalDeleteOutcome;

/// Successful deletion paired with its caller-visible target.
#[derive(Debug)]
pub struct LocalDeleteOperationResult {
    /// Stores the path associated with this value.
    pub path: PathBuf,
    /// Stores the directory associated with this value.
    pub directory: bool,
    /// Stores the outcome associated with this value.
    pub outcome: LocalDeleteOutcome,
    /// Exact count when known; a missing entry accepted by `missing_ok` is
    /// reported as zero, while successful recursive deletion reports `None`
    /// because the core exposes only whether the requested root was removed.
    pub deleted_entries: Option<u64>,
}
