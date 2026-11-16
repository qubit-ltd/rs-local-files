// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::options::LocalRenameOptions;

/// Owned paths and policy for a rename operation.
#[derive(Clone, Debug)]
pub struct LocalRenameRequest {
    /// Stores the source associated with this value.
    pub source: PathBuf,
    /// Stores the destination associated with this value.
    pub destination: PathBuf,
    /// Stores the options associated with this value.
    pub options: LocalRenameOptions,
}
