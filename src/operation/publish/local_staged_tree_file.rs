// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

/// One owned file prepared for a staged-tree publication.
#[derive(Debug)]
pub struct LocalStagedTreeFile {
    /// Normalized relative path beneath the staging directory.
    pub path: PathBuf,
    /// Owned bytes written to this path.
    pub bytes: Vec<u8>,
}
