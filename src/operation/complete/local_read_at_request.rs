// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::SeekFrom;
use std::path::PathBuf;

use crate::options::LocalReadOptions;

/// Owned input to a positional read on one opened file object.
#[derive(Debug)]
pub struct LocalReadAtRequest {
    /// Stores the path associated with this value.
    pub path: PathBuf,
    /// Stores the read associated with this value.
    pub read: LocalReadOptions,
    /// Stores the offset associated with this value.
    pub offset: SeekFrom,
    /// Stores the max bytes associated with this value.
    pub max_bytes: usize,
}
