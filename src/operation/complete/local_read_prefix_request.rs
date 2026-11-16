// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::options::LocalReadOptions;

/// Owned input to a bounded prefix read.
#[derive(Debug)]
pub struct LocalReadPrefixRequest {
    /// Stores the path associated with this value.
    pub path: PathBuf,
    /// Stores the read associated with this value.
    pub read: LocalReadOptions,
    /// Stores the max bytes associated with this value.
    pub max_bytes: usize,
}
