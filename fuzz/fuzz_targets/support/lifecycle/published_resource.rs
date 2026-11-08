// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Published artifacts retained for later integrity checks.

use std::path::PathBuf;

use super::DirectorySnapshot;

/// Published artifacts whose contents must survive later operations.
pub(super) enum PublishedResource {
    /// Published file path and exact bytes expected to remain there.
    File(PathBuf, Vec<u8>),
    /// Published directory path and captured tree state.
    Directory(PathBuf, DirectorySnapshot),
}
