// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::options::LocalDeleteOptions;

/// Owned path and policy for file or directory deletion.
#[derive(Clone, Debug)]
pub struct LocalDeleteRequest {
    /// Stores the path associated with this value.
    pub path: PathBuf,
    /// Stores the options associated with this value.
    pub options: LocalDeleteOptions,
    /// Whether the target must be a directory and recursive policy applies.
    pub directory: bool,
}
