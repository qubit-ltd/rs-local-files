// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use super::LocalStagedTreeFile;

/// Owned request that creates, verifies, and publishes a staged tree.
#[derive(Debug)]
pub struct LocalStagedTreePublishRequest {
    /// Parent directory opened through the facade's captured authority.
    /// Parent directory opened under the facade authority.
    pub parent: PathBuf,
    /// Single native entry name installed under `parent`.
    /// Single native entry name for the published tree.
    pub name: std::ffi::OsString,
    /// Private temporary-directory creation policy.
    /// Creation policy applied to the private staging directory.
    pub stage_options: crate::options::LocalTempDirectoryOptions,
    /// Files written under the stage; paths must be direct children.
    /// Files created beneath the stage before publication.
    pub files: Vec<LocalStagedTreeFile>,
    /// Durability and access options for publication; exact verification is
    /// added automatically.
    /// Durability and access policy used when installing the completed tree.
    pub publish_options: crate::options::LocalTreePublishOptions,
}
