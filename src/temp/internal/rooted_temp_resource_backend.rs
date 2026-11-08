// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Temporary-resource behavior is covered through public integration tests.
//! Root-descriptor-bound temporary-resource storage.

use std::path::PathBuf;
use std::sync::Arc;

use crate::rooted::Metadata;
use crate::rooted::Root;

/// Retains the root authority and root-relative locations needed to manage a
/// temporary descendant.
///
/// The captured identity lets cleanup detect ordinary replacement of the
/// descendant before removal. The open root handle remains the authority for
/// both the descendant and its private cleanup sandbox; stored paths only
/// identify entries relative to that handle.
#[derive(Debug)]
pub(crate) struct RootedTempResourceBackend {
    /// Unique identity captured through the retained root authority.
    pub(crate) identity: Metadata,
    /// Open root descriptor/handle; its diagnostic path is never authority.
    pub(crate) root: Arc<Root>,
    /// Root-relative descendant path.
    pub(crate) relative_path: PathBuf,
    /// Root-relative private cleanup sandbox.
    pub(crate) sandbox_path: PathBuf,
}
