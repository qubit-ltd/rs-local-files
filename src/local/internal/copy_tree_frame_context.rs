// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native path coordinates and logical depth for one copy traversal entry.

use std::path::PathBuf;

/// Owned coordinates constructed once for an entry and shared with its backend.
#[derive(Clone, Debug)]
pub(crate) struct CopyTreeFrameContext {
    /// Backend-native source path; Rooted paths stay relative to its authority.
    pub(crate) source: PathBuf,
    /// Backend-native destination path for errors and publication operations.
    pub(crate) destination: PathBuf,
    /// Logical depth below the requested root, whose depth is zero.
    pub(crate) depth: usize,
}
