// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::options::LocalReadAllOptions;

/// Owned input to a complete local file read.
#[derive(Debug)]
pub struct LocalReadRequest {
    /// Path interpreted using the filesystem authority captured at submission.
    pub path: PathBuf,
    /// Complete-read behavior and byte limit.
    pub read: LocalReadAllOptions,
}
