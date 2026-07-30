// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Temporary-resource behavior is covered through public integration tests.
//! Root-descriptor-bound temporary-resource storage.

use std::{
    path::PathBuf,
    sync::Arc,
};

/// Retains the exact root authority used to create a temporary descendant.
#[derive(Debug)]
pub(crate) struct RootedTempResourceBackend {
    /// Open root descriptor/handle; its diagnostic path is never authority.
    pub(crate) root: Arc<crate::rooted::Root>,
    /// Root-relative descendant path.
    pub(crate) relative_path: PathBuf,
}
