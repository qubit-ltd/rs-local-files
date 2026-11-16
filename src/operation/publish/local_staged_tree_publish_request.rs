// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

/// Owned request that creates, verifies, and publishes a staged tree.
#[derive(Debug)]
pub struct LocalStagedTreePublishRequest {
    /// Parent directory opened through the facade's captured authority.
    pub parent: PathBuf,
    /// Single native entry name installed under `parent`.
    pub name: std::ffi::OsString,
    /// Private temporary-directory creation policy.
    pub stage_options: crate::options::LocalTempDirectoryOptions,
    /// Files written under the stage; paths must be direct children.
    pub files: Vec<LocalStagedTreeFile>,
    /// Durability and access options for publication; exact verification is
    /// added automatically.
    pub publish_options: crate::options::LocalTreePublishOptions,
}
