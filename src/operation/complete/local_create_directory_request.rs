// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::options::LocalCreateDirectoryOptions;

/// Owned path and policy for directory creation.
#[derive(Clone, Debug)]
pub struct LocalCreateDirectoryRequest {
    /// Stores the path associated with this value.
    pub path: PathBuf,
    /// Stores the options associated with this value.
    pub options: LocalCreateDirectoryOptions,
}
